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
//! 本矩阵不校验标定数值是否可信（`Trust` 与四维门在 [`crate::motor::service_gate`]）、不校验编码器帧
//! 是否可用（[`crate::motor::service_encoder`]）、不决定故障是否成立（prio 7 中断与窗口监管者）。
//! 它只回答"在这个状态下，这个事件允不允许发生，允许的话输出什么"。

use contracts::joint::protocol_fsm::{LoopAction, MotorState, Reject, SysEvent};
use contracts::{CalibStep, Cmd, Mode};

use crate::motor::config_policy as policy;

#[cfg(test)]
mod tests;

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
            Some(next) => Ok((S::Calibrating(next), calib_step_action(next))),
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

/// 辅助函数：根据标定步骤决定允许的物理输出权限。
///
/// C0 零位测量与 C6 冻结阶段六路全低，严禁发波；C1–C5 的极性/方向/增益/偏置/一致性
/// 辨识才允许受控开环矢量强拖。所有返回 `Calibrating(step)` 的分支必须经过本函数，
/// 不允许在调用点各写一份 `match`（否则 C0 很容易被写成 `OpenLoop`）。
const fn calib_step_action(step: CalibStep) -> LoopAction {
    match step {
        // C0 零位测量与 C6 冻结阶段：六路全低，严禁发波
        CalibStep::C0Zero | CalibStep::C6Freeze => LoopAction::Coast,
        // C1~C5 极性与增益辨识阶段：允许受控开环矢量强拖
        CalibStep::C1Polarity
        | CalibStep::C2Direction
        | CalibStep::C3GainAccum
        | CalibStep::C4Bias
        | CalibStep::C5Consistency => LoopAction::OpenLoop,
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
        Cmd::SetMode(Mode::Calibrate) => Ok((
            S::Calibrating(CalibStep::C0Zero),
            calib_step_action(CalibStep::C0Zero),
        )),
        Cmd::SetMode(Mode::Run) => closed_loop_entry(record_valid),
        Cmd::SetMode(Mode::Bench) => Ok((S::Bench, LoopAction::OpenLoop)),
        Cmd::SetMode(Mode::SelfTest | Mode::Fault) => Err(Reject::UnsupportedMode),
        Cmd::SetTarget(_) => Err(Reject::NotRunning),
        Cmd::Bench { .. } => Err(Reject::NotRunning),
        Cmd::CommitCalib(_) => Err(Reject::InvalidTransition),
        Cmd::ClearFault => Err(Reject::UnroutedClearFault),
    }
}

/// 标定中的命令表：允许中止（回空闲）、重复请求标定（保持当前步）与在 C6 提交。
const fn calibrating_cmd(now: CalibStep, cmd: Cmd) -> Result<(MotorState, LoopAction), Reject> {
    use MotorState as S;

    match cmd {
        Cmd::SetMode(Mode::Standby) => Ok((S::Idle, LoopAction::Coast)),
        // 重复请求标定 = 保持当前步骤：不能因为一条命令把 C0/C6 的静默阶段重新开波。
        Cmd::SetMode(Mode::Calibrate) => Ok((S::Calibrating(now), calib_step_action(now))),
        Cmd::SetMode(Mode::Run | Mode::Bench) => Err(Reject::MotorBusy),
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
