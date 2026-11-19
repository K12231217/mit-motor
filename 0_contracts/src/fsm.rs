//! 电机状态机的类型：只放数据，不放规则。
//!
//! 迁移规则是策略，实现与全表穷举在 `user::fsm`；本模块只固定"有哪些状态、有哪些
//! 事件、拒绝原因怎么编号"，因为它们跨 crate 传递并进日志与遥测。
//!
//! # 与线上契约的关系
//!
//! 当前状态不进状态字：状态字已有模式（bit0–3）、标定状态（bit4–6）、故障码（bit16–23），
//! 三者合起来能重建 [`MotorState`] 的粗粒度投影。额外的"标定档案是否有效"是上电从
//! bank 2 加载的**事实**，不是状态，因此作为 `user::fsm::step` 的参数而不是变体——否则
//! `Idle` 与"是否已标定"会各存一份，两处可以互相矛盾。

use crate::events::Cmd;
use crate::faults::FaultCode;
use crate::types::CalibStep;

/// 电机当前所处的阶段。
///
/// 与 [`crate::types::Mode`] 的区别：`Mode` 是主机请求的目标模式，可能尚未生效；
/// 本枚举是机器实际停在的阶段。`BootCheck` 与"运行时切入 `Fault`"只有本枚举能表达。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotorState {
    /// 上电自检 S0–S5 尚未出结果，除自检结果与硬件故障外拒绝一切事件。
    BootCheck,
    /// 空闲：输出高阻，可接受模式切换与标定。
    Idle,
    /// 标定中，携带当前步骤。
    ///
    /// 步骤推进由标定任务经 [`SysEvent::CalibStepCompleted`] 通知；判据失败回退或锁存
    /// 属于标定任务自身，不在这里表达。
    Calibrating(CalibStep),
    /// 闭环运行，20 kHz 电流环。
    ClosedLoop,
    /// 台架单相注入，闭环关闭。
    Bench,
    /// 故障锁存，携带**首个**故障码；后续故障不覆盖已锁存的那个。
    Fault(FaultCode),
}

/// 20 kHz 快环本轮应输出的种类。
///
/// 只选"哪一类输出"，不携带数值：`dq` 目标来自 [`crate::snapshot::CmdView`]，电压矢量角来自
/// 标定与台架各自的斜坡发生器。把数值塞进本枚举会造出第二份目标真源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopAction {
    /// 六路输入全低，高阻。
    Coast,
    /// 开环电压矢量；标定强拖与台架注入共用，区别只在斜坡发生器。
    OpenLoop,
    /// `dq` 电流环。
    Current,
}

/// 触发状态机的事件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SysEvent {
    /// 上电自检 S0–S5 全过。
    SelfTestPassed,
    /// 上电自检失败，携带失败项的故障码。
    SelfTestFailed(FaultCode),
    /// 硬件故障已锁存，由 prio 7 的 `BKIN` / `nFault` 中断投递。
    HwFault(FaultCode),
    /// 标定任务的当前步骤完成。
    CalibStepCompleted,
    /// 清除故障，携带距最后一次故障的时长，单位 `µs`。
    ///
    /// 时长进事件而不是让状态机读时钟：`step` 因此保持纯函数，无故障窗口可被穷举测试，
    /// 也杜绝"没有证据就清故障"的写法。
    ClearFault {
        /// 距最后一次故障的时长，单位 `µs`。
        fault_free_us: u32,
    },
    /// 下行命令。
    ///
    /// [`Cmd::ClearFault`] 不得以此路由：裸命令没有窗口证据，一律
    /// [`Reject::UnroutedClearFault`]。
    Cmd(Cmd),
}

/// 迁移被拒绝的原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Reject {
    /// 上电自检尚未通过。
    BeforeSelfTest = 0,
    /// 自检结果重复上报。
    AlreadyBooted = 1,
    /// 无有效标定档案时请求闭环。
    NotCalibrated = 2,
    /// 该状态下不允许此切换：运行中禁切标定、标定中禁切台架。
    MotorBusy = 3,
    /// 故障锁存中，只接受带窗口证据的 [`SysEvent::ClearFault`]。
    FaultLatched = 4,
    /// 无故障窗口不足。
    FaultStillActive = 5,
    /// 标定未到 C6 就提交档案。
    CalibNotFrozen = 6,
    /// C6 之后仍收到步骤完成。
    CalibAlreadyFrozen = 7,
    /// 该状态不接受目标值或台架注入。
    NotRunning = 8,
    /// 裸 [`Cmd::ClearFault`]：必须由故障监管者带 `fault_free_us` 重投。
    UnroutedClearFault = 9,
    /// 该模式不能被命令进入：自检与故障只由机器自己进入。
    UnsupportedMode = 10,
    /// 其余不合法组合。
    InvalidTransition = 11,
}

impl Reject {
    /// 编号，供日志与遥测。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

// 状态只做选择，不许变胖：带 payload 的是 `CalibStep` 与 `FaultCode`（各 1 字节）。
const _: () = assert!(core::mem::size_of::<MotorState>() <= 4);
const _: () = assert!(core::mem::size_of::<LoopAction>() == 1);
const _: () = assert!(core::mem::size_of::<Reject>() == 1);
