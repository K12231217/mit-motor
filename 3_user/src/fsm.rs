//! 电机状态机的迁移矩阵：纯函数，无状态，宿主机全表穷举。
//!
//! 规则只有一处实现（本文件），调用方只许调用 `step`，不许自己判断"能不能切"。
//! 输入是当前状态、事件与"标定档案是否有效"，输出是新状态与 20 kHz 快环动作；没有
//! 时钟、没有全局量，因此 `state × event × record_valid` 可以在宿主机上跑满。
//!
//! # 为什么拒绝原因要显式
//!
//! 每条拒绝都对应一个可以写错的具体分支（未标定就闭环、运行中切标定、没有窗口就清故障）。
//! 返回 `Result` 而不是"静默保持原状态"，是为了让错误在测试里可见、在日志里可查、
//! 在状态字的 `cmd_rejected` 位上可观测。
//!
//! # 边界
//!
//! 本矩阵不校验标定数值是否可信（`Trust` 与四维门在 [`crate::gate`]）、不校验编码器帧
//! 是否可用（[`crate::encoder`]）、不决定故障是否成立（prio 7 中断与窗口监管者）。
//! 它只回答"在这个状态下，这个事件允不允许发生，允许的话输出什么"。

use contracts::fsm::{LoopAction, MotorState, Reject, SysEvent};
use contracts::{CalibStep, Cmd, Mode};

use crate::policy;

/// 迁移矩阵：给定当前状态、事件与标定档案有效性，返回新状态与快环动作。
///
/// `record_valid` 是上电从 bank 2 加载标定档案的结果，只在"进入闭环"这一格被使用；
/// 它不属于状态，因为它在标定提交与重启之外不会变，塞进状态会让状态数翻倍。
///
/// # Errors
///
/// - [`Reject::BeforeSelfTest`]：`BootCheck` 下收到自检结果以外的事件。
/// - [`Reject::AlreadyBooted`]：非 `BootCheck` 状态下再收到自检结果。
/// - [`Reject::NotCalibrated`]：`record_valid == false` 时请求闭环。
/// - [`Reject::MotorBusy`]：闭环中切标定或台架、标定中切台架或闭环、台架中切标定，
///   以及在运行或台架状态下提交标定档案。
/// - [`Reject::FaultLatched`]：故障锁存中切换模式或提交档案。
/// - [`Reject::FaultStillActive`]：`ClearFault` 的 `fault_free_us` 小于
///   [`policy::FAULT_CLEAR_HOLD_US`]。
/// - [`Reject::CalibNotFrozen`]：标定未到 `C6Freeze` 就提交档案。
/// - [`Reject::CalibAlreadyFrozen`]：`C6Freeze` 之后仍收到步骤完成。
/// - [`Reject::NotRunning`]：非闭环状态下收到目标值，或非台架状态下收到台架注入。
/// - [`Reject::UnroutedClearFault`]：裸 [`Cmd::ClearFault`]，没有无故障窗口证据。
/// - [`Reject::UnsupportedMode`]：命令请求自检或故障模式，二者只由机器自己进入。
/// - [`Reject::InvalidTransition`]：其余不合法组合。
pub const fn step(
    cur: MotorState,
    evt: SysEvent,
    record_valid: bool,
) -> Result<(MotorState, LoopAction), Reject> {
    use MotorState as S;

    match (cur, evt) {
        // 硬件故障优先于一切。已锁存时首个故障码胜出：锁存之后再来别的故障不覆盖它，
        // 否则现场只剩"最后一个报上来的"，最先出事的那个证据就丢了。
        (S::Fault(first), SysEvent::HwFault(_)) => Ok((S::Fault(first), LoopAction::Coast)),
        (_, SysEvent::HwFault(code)) => Ok((S::Fault(code), LoopAction::Coast)),

        // 上电自检只出一次结果，之后不再改写状态。
        (S::BootCheck, SysEvent::SelfTestPassed) => Ok((S::Idle, LoopAction::Coast)),
        (S::BootCheck, SysEvent::SelfTestFailed(code)) => Ok((S::Fault(code), LoopAction::Coast)),
        (S::BootCheck, _) => Err(Reject::BeforeSelfTest),
        (_, SysEvent::SelfTestPassed | SysEvent::SelfTestFailed(_)) => Err(Reject::AlreadyBooted),

        (S::Idle, SysEvent::Cmd(cmd)) => idle_cmd(cmd, record_valid),
        (S::Idle, SysEvent::CalibStepCompleted | SysEvent::ClearFault { .. }) => {
            Err(Reject::InvalidTransition)
        }

        (S::Calibrating(now), SysEvent::Cmd(cmd)) => calibrating_cmd(now, cmd),
        (S::Calibrating(now), SysEvent::CalibStepCompleted) => match now.next() {
            Some(next) => Ok((S::Calibrating(next), LoopAction::OpenLoop)),
            None => Err(Reject::CalibAlreadyFrozen),
        },
        (S::Calibrating(_), SysEvent::ClearFault { .. }) => Err(Reject::InvalidTransition),

        (S::ClosedLoop, SysEvent::Cmd(cmd)) => closed_loop_cmd(cmd, record_valid),
        (S::ClosedLoop, SysEvent::CalibStepCompleted | SysEvent::ClearFault { .. }) => {
            Err(Reject::InvalidTransition)
        }

        (S::Bench, SysEvent::Cmd(cmd)) => bench_cmd(cmd, record_valid),
        (S::Bench, SysEvent::CalibStepCompleted | SysEvent::ClearFault { .. }) => {
            Err(Reject::InvalidTransition)
        }

        // 故障锁存：唯一出口是带窗口证据的清故障。
        (S::Fault(_), SysEvent::Cmd(Cmd::ClearFault)) => Err(Reject::UnroutedClearFault),
        (S::Fault(_), SysEvent::Cmd(_)) => Err(Reject::FaultLatched),
        (S::Fault(_), SysEvent::ClearFault { fault_free_us }) => {
            if fault_free_us >= policy::FAULT_CLEAR_HOLD_US {
                Ok((S::Idle, LoopAction::Coast))
            } else {
                Err(Reject::FaultStillActive)
            }
        }
        (S::Fault(_), SysEvent::CalibStepCompleted) => Err(Reject::InvalidTransition),
    }
}

/// 进入闭环的唯一入口：档案无效就拒绝，不靠调用方记得先查。
const fn closed_loop_entry(record_valid: bool) -> Result<(MotorState, LoopAction), Reject> {
    if record_valid {
        Ok((MotorState::ClosedLoop, LoopAction::Current))
    } else {
        Err(Reject::NotCalibrated)
    }
}

/// 空闲状态下的命令表。
const fn idle_cmd(cmd: Cmd, record_valid: bool) -> Result<(MotorState, LoopAction), Reject> {
    use MotorState as S;

    match cmd {
        Cmd::SetMode(Mode::Standby) => Ok((S::Idle, LoopAction::Coast)),
        Cmd::SetMode(Mode::Calibrate) => {
            Ok((S::Calibrating(CalibStep::C0Zero), LoopAction::OpenLoop))
        }
        Cmd::SetMode(Mode::Run) => closed_loop_entry(record_valid),
        Cmd::SetMode(Mode::Bench) => Ok((S::Bench, LoopAction::OpenLoop)),
        Cmd::SetMode(Mode::SelfTest | Mode::Fault) => Err(Reject::UnsupportedMode),
        Cmd::SetTarget(_) => Err(Reject::NotRunning),
        Cmd::Bench { .. } => Err(Reject::NotRunning),
        Cmd::CommitCalib(_) => Err(Reject::InvalidTransition),
        Cmd::ClearFault => Err(Reject::UnroutedClearFault),
    }
}

/// 标定中的命令表：只允许中止（回空闲）与在 C6 提交。
const fn calibrating_cmd(now: CalibStep, cmd: Cmd) -> Result<(MotorState, LoopAction), Reject> {
    use MotorState as S;

    match cmd {
        Cmd::SetMode(Mode::Standby) => Ok((S::Idle, LoopAction::Coast)),
        Cmd::SetMode(Mode::Calibrate | Mode::Run | Mode::Bench) => Err(Reject::MotorBusy),
        Cmd::SetMode(Mode::SelfTest | Mode::Fault) => Err(Reject::UnsupportedMode),
        Cmd::CommitCalib(_) => match now {
            CalibStep::C6Freeze => Ok((S::Idle, LoopAction::Coast)),
            _ => Err(Reject::CalibNotFrozen),
        },
        Cmd::SetTarget(_) | Cmd::Bench { .. } => Err(Reject::NotRunning),
        Cmd::ClearFault => Err(Reject::UnroutedClearFault),
    }
}

/// 闭环中的命令表：运行中禁切标定与台架，禁提交档案。
const fn closed_loop_cmd(cmd: Cmd, record_valid: bool) -> Result<(MotorState, LoopAction), Reject> {
    use MotorState as S;

    match cmd {
        Cmd::SetMode(Mode::Standby) => Ok((S::Idle, LoopAction::Coast)),
        Cmd::SetMode(Mode::Run) => closed_loop_entry(record_valid),
        Cmd::SetMode(Mode::Calibrate | Mode::Bench) => Err(Reject::MotorBusy),
        Cmd::SetMode(Mode::SelfTest | Mode::Fault) => Err(Reject::UnsupportedMode),
        Cmd::SetTarget(_) => Ok((S::ClosedLoop, LoopAction::Current)),
        Cmd::Bench { .. } => Err(Reject::MotorBusy),
        Cmd::CommitCalib(_) => Err(Reject::MotorBusy),
        Cmd::ClearFault => Err(Reject::UnroutedClearFault),
    }
}

/// 台架中的命令表：只接受单相注入与回空闲。
const fn bench_cmd(cmd: Cmd, record_valid: bool) -> Result<(MotorState, LoopAction), Reject> {
    use MotorState as S;

    match cmd {
        Cmd::SetMode(Mode::Standby) => Ok((S::Idle, LoopAction::Coast)),
        Cmd::SetMode(Mode::Run) => closed_loop_entry(record_valid),
        Cmd::SetMode(Mode::Bench) => Ok((S::Bench, LoopAction::OpenLoop)),
        Cmd::SetMode(Mode::Calibrate) => Err(Reject::MotorBusy),
        Cmd::SetMode(Mode::SelfTest | Mode::Fault) => Err(Reject::UnsupportedMode),
        Cmd::Bench { .. } => Ok((S::Bench, LoopAction::OpenLoop)),
        Cmd::SetTarget(_) => Err(Reject::NotRunning),
        Cmd::CommitCalib(_) => Err(Reject::MotorBusy),
        Cmd::ClearFault => Err(Reject::UnroutedClearFault),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::faults::FaultCode;
    use contracts::{CalibPayload, Cmd, DqTarget};

    /// 覆盖全部 6 个变体；`Calibrating` 另按步骤单独遍历。
    const ALL_STATES: [MotorState; 7] = [
        MotorState::BootCheck,
        MotorState::Idle,
        MotorState::Calibrating(CalibStep::C0Zero),
        MotorState::Calibrating(CalibStep::C6Freeze),
        MotorState::ClosedLoop,
        MotorState::Bench,
        MotorState::Fault(FaultCode::OverCurrent),
    ];

    const ALL_REJECTS: [Reject; 12] = [
        Reject::BeforeSelfTest,
        Reject::AlreadyBooted,
        Reject::NotCalibrated,
        Reject::MotorBusy,
        Reject::FaultLatched,
        Reject::FaultStillActive,
        Reject::CalibNotFrozen,
        Reject::CalibAlreadyFrozen,
        Reject::NotRunning,
        Reject::UnroutedClearFault,
        Reject::UnsupportedMode,
        Reject::InvalidTransition,
    ];

    fn target_cmd() -> Cmd {
        Cmd::SetTarget(DqTarget {
            d_amps: 0.0,
            q_amps: 0.4,
        })
    }

    fn payload() -> CalibPayload {
        CalibPayload {
            zero_ma: [2_048, 2_044, 2_051],
            gain_q14: [16_384, 16_384],
            crc: 0,
        }
    }

    /// 事件的代表样本：每个变体至少一次，`SetMode` 覆盖全部 6 个模式。
    fn events() -> std::vec::Vec<SysEvent> {
        let mut out = std::vec![
            SysEvent::SelfTestPassed,
            SysEvent::SelfTestFailed(FaultCode::AdcTimeout),
            SysEvent::HwFault(FaultCode::OverCurrent),
            SysEvent::CalibStepCompleted,
            SysEvent::ClearFault { fault_free_us: 0 },
            SysEvent::ClearFault {
                fault_free_us: policy::FAULT_CLEAR_HOLD_US,
            },
        ];
        for mode in [
            Mode::SelfTest,
            Mode::Standby,
            Mode::Calibrate,
            Mode::Run,
            Mode::Bench,
            Mode::Fault,
        ] {
            out.push(SysEvent::Cmd(Cmd::SetMode(mode)));
        }
        out.push(SysEvent::Cmd(target_cmd()));
        out.push(SysEvent::Cmd(Cmd::Bench {
            phase_sel: 0,
            amps: 0.5,
        }));
        out.push(SysEvent::Cmd(Cmd::CommitCalib(payload())));
        out.push(SysEvent::Cmd(Cmd::ClearFault));
        out
    }

    #[test]
    fn hw_fault_preempts_every_state_and_keeps_the_first_code() {
        let code = FaultCode::OverCurrent;
        for state in ALL_STATES {
            let want = match state {
                MotorState::Fault(first) => Ok((MotorState::Fault(first), LoopAction::Coast)),
                _ => Ok((MotorState::Fault(code), LoopAction::Coast)),
            };
            for record_valid in [false, true] {
                assert_eq!(
                    step(state, SysEvent::HwFault(code), record_valid),
                    want,
                    "{state:?}"
                );
            }
        }
    }

    #[test]
    fn action_always_matches_the_resulting_state() {
        for state in ALL_STATES {
            for evt in events() {
                for record_valid in [false, true] {
                    let Ok((next, action)) = step(state, evt, record_valid) else {
                        continue;
                    };
                    let want = match next {
                        MotorState::Idle | MotorState::Fault(_) => LoopAction::Coast,
                        MotorState::Calibrating(_) | MotorState::Bench => LoopAction::OpenLoop,
                        MotorState::ClosedLoop => LoopAction::Current,
                        // `BootCheck` 只能由复位进入，迁移不许回到它。
                        MotorState::BootCheck => panic!("{state:?} + {evt:?} 回到了 BootCheck"),
                    };
                    assert_eq!(action, want, "{state:?} + {evt:?} → {next:?}");
                }
            }
        }
    }

    #[test]
    fn closed_loop_requires_a_calibration_record() {
        let run = SysEvent::Cmd(Cmd::SetMode(Mode::Run));
        let idle = MotorState::Idle;
        assert_eq!(step(idle, run, false), Err(Reject::NotCalibrated));
        assert_eq!(
            step(idle, run, true),
            Ok((MotorState::ClosedLoop, LoopAction::Current))
        );
        // 台架同样受档案约束：注入也要能进闭环才有意义。
        let bench = MotorState::Bench;
        assert_eq!(step(bench, run, false), Err(Reject::NotCalibrated));
        assert_eq!(
            step(bench, run, true),
            Ok((MotorState::ClosedLoop, LoopAction::Current))
        );
    }

    #[test]
    fn running_never_switches_to_calibration_or_bench() {
        let busy = MotorState::ClosedLoop;
        for cmd in [
            Cmd::SetMode(Mode::Calibrate),
            Cmd::SetMode(Mode::Bench),
            Cmd::CommitCalib(payload()),
        ] {
            assert_eq!(
                step(busy, SysEvent::Cmd(cmd), true),
                Err(Reject::MotorBusy),
                "{cmd:?}"
            );
        }
        // 中止后回到空闲，档案不动，因此可以直接再进闭环。
        assert_eq!(
            step(busy, SysEvent::Cmd(Cmd::SetMode(Mode::Standby)), true),
            Ok((MotorState::Idle, LoopAction::Coast))
        );
    }

    #[test]
    fn committing_a_record_is_only_legal_at_c6() {
        let commit = SysEvent::Cmd(Cmd::CommitCalib(payload()));
        let mut cursor = CalibStep::from_u8(0);
        let mut seen = 0;
        while let Some(now) = cursor {
            let got = step(MotorState::Calibrating(now), commit, true);
            if now.as_u8() == CalibStep::C6Freeze.as_u8() {
                assert_eq!(got, Ok((MotorState::Idle, LoopAction::Coast)));
            } else {
                assert_eq!(got, Err(Reject::CalibNotFrozen), "{now:?}");
            }
            seen += 1;
            cursor = now.next();
        }
        assert_eq!(seen, 7);
    }

    #[test]
    fn calib_steps_advance_once_and_stop_at_c6() {
        let done = SysEvent::CalibStepCompleted;
        let mut cursor = CalibStep::from_u8(0);
        while let Some(now) = cursor {
            let got = step(MotorState::Calibrating(now), done, true);
            match now.next() {
                Some(next) => assert_eq!(
                    got,
                    Ok((MotorState::Calibrating(next), LoopAction::OpenLoop)),
                    "{now:?}"
                ),
                None => assert_eq!(got, Err(Reject::CalibAlreadyFrozen)),
            }
            cursor = now.next();
        }
    }

    #[test]
    fn clearing_a_fault_needs_the_full_quiet_window() {
        let faulted = MotorState::Fault(FaultCode::OverCurrent);
        let hold = policy::FAULT_CLEAR_HOLD_US;
        for too_soon in [0, hold - 1] {
            assert_eq!(
                step(
                    faulted,
                    SysEvent::ClearFault {
                        fault_free_us: too_soon
                    },
                    true
                ),
                Err(Reject::FaultStillActive),
                "{too_soon} µs"
            );
        }
        assert_eq!(
            step(
                faulted,
                SysEvent::ClearFault {
                    fault_free_us: hold
                },
                true
            ),
            Ok((MotorState::Idle, LoopAction::Coast))
        );
        // 裸命令没有窗口证据，一律拒绝：清故障只认监管者投递的事件。
        assert_eq!(
            step(faulted, SysEvent::Cmd(Cmd::ClearFault), true),
            Err(Reject::UnroutedClearFault)
        );
        // 清故障不改档案：窗口满足后仍可直接进闭环。
        assert_eq!(
            step(
                MotorState::Idle,
                SysEvent::Cmd(Cmd::SetMode(Mode::Run)),
                true
            ),
            Ok((MotorState::ClosedLoop, LoopAction::Current))
        );
    }

    #[test]
    fn self_test_result_is_accepted_only_at_boot() {
        for evt in events() {
            match evt {
                SysEvent::SelfTestPassed | SysEvent::SelfTestFailed(_) | SysEvent::HwFault(_) => {}
                _ => assert_eq!(
                    step(MotorState::BootCheck, evt, true),
                    Err(Reject::BeforeSelfTest),
                    "{evt:?}"
                ),
            }
        }
        for state in [
            MotorState::Idle,
            MotorState::ClosedLoop,
            MotorState::Bench,
            MotorState::Calibrating(CalibStep::C0Zero),
            MotorState::Fault(FaultCode::OverCurrent),
        ] {
            assert_eq!(
                step(state, SysEvent::SelfTestPassed, true),
                Err(Reject::AlreadyBooted),
                "{state:?}"
            );
        }
        assert_eq!(
            step(
                MotorState::BootCheck,
                SysEvent::SelfTestFailed(FaultCode::HardFault),
                true
            ),
            Ok((MotorState::Fault(FaultCode::HardFault), LoopAction::Coast))
        );
    }

    #[test]
    fn every_state_and_reject_is_reachable_and_boot_is_not() {
        // 状态可达性走闭包：Calibrating(C6) 只能由 C5 推进一步得到，单步扫描看不到它。
        let mut frontier = std::vec::Vec::new();
        let mut reached = std::vec::Vec::new();
        frontier.push(MotorState::BootCheck);
        reached.push(MotorState::BootCheck);
        let mut guard = 0u32;
        while let Some(seed) = frontier.pop() {
            guard += 1;
            assert!(guard < 10_000, "迁移闭包不收敛");
            for evt in events() {
                for record_valid in [false, true] {
                    let Ok((next, _)) = step(seed, evt, record_valid) else {
                        continue;
                    };
                    if !reached.contains(&next) {
                        reached.push(next);
                        frontier.push(next);
                    }
                }
            }
        }
        for want in ALL_STATES {
            if want == MotorState::BootCheck {
                continue;
            }
            assert!(reached.contains(&want), "{want:?} 从 BootCheck 不可达");
        }

        // 每个拒绝原因都必须有一条真实路径产生它，否则就是死变体。
        let mut rejects = std::vec::Vec::new();
        for state in ALL_STATES {
            for evt in events() {
                for record_valid in [false, true] {
                    if let Err(reject) = step(state, evt, record_valid) {
                        rejects.push(reject);
                    }
                }
            }
        }
        for want in ALL_REJECTS {
            assert!(rejects.contains(&want), "{want:?} 从未产生");
        }
    }
}
