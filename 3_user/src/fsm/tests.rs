//! 状态 × 事件迁移矩阵的全表穷举测试。

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
                    MotorState::Calibrating(step) => calib_step_action(step),
                    MotorState::Bench => LoopAction::OpenLoop,
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
                Ok((MotorState::Calibrating(next), calib_step_action(next))),
                "{now:?}"
            ),
            None => assert_eq!(got, Err(Reject::CalibAlreadyFrozen)),
        }
        cursor = now.next();
    }
}

/// 进入标定即为 C0：不得开波；C5→C6 的推进同样回到 Coast。C1–C5 才允许开环。
#[test]
fn calibration_never_opens_the_bridge_at_c0_or_c6() {
    assert_eq!(
        step(
            MotorState::Idle,
            SysEvent::Cmd(Cmd::SetMode(Mode::Calibrate)),
            true
        ),
        Ok((
            MotorState::Calibrating(CalibStep::C0Zero),
            LoopAction::Coast
        ))
    );
    assert_eq!(
        step(
            MotorState::Calibrating(CalibStep::C5Consistency),
            SysEvent::CalibStepCompleted,
            true
        ),
        Ok((
            MotorState::Calibrating(CalibStep::C6Freeze),
            LoopAction::Coast
        ))
    );
    for (now, next) in [
        (CalibStep::C0Zero, CalibStep::C1Polarity),
        (CalibStep::C1Polarity, CalibStep::C2Direction),
        (CalibStep::C2Direction, CalibStep::C3GainAccum),
        (CalibStep::C3GainAccum, CalibStep::C4Bias),
        (CalibStep::C4Bias, CalibStep::C5Consistency),
    ] {
        assert_eq!(
            step(
                MotorState::Calibrating(now),
                SysEvent::CalibStepCompleted,
                true
            ),
            Ok((MotorState::Calibrating(next), LoopAction::OpenLoop)),
            "{now:?} → {next:?}"
        );
    }

    // 重复下发 Calibrate 只保持当前步：C0/C6 仍必须 Coast。
    for now in [CalibStep::C0Zero, CalibStep::C6Freeze] {
        assert_eq!(
            step(
                MotorState::Calibrating(now),
                SysEvent::Cmd(Cmd::SetMode(Mode::Calibrate)),
                true
            ),
            Ok((MotorState::Calibrating(now), LoopAction::Coast)),
            "{now:?}"
        );
    }
    assert_eq!(
        step(
            MotorState::Calibrating(CalibStep::C3GainAccum),
            SysEvent::Cmd(Cmd::SetMode(Mode::Calibrate)),
            true
        ),
        Ok((
            MotorState::Calibrating(CalibStep::C3GainAccum),
            LoopAction::OpenLoop
        ))
    );
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
