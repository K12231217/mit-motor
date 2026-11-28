//! 独立监督：任务心跳与通信超时的纯状态机。
//!
//! 不读时钟、不碰寄存器、不起线程：调用方每拍把 `now_us` 与心跳喂进来，本模块只做
//! “是否超窗 → 锁存哪个故障”的判定。超窗后不会自动恢复，必须显式 [`Supervisor::clear`]；
//! 这与 `crate::joint::service_fsm` 的故障锁存语义一致，避免“通信刚恢复就自动使能”。
//!
//! 时间全部是 `u32` 微秒并允许回绕；窗口 ≤ 20 ms，远小于 71 min 的回绕周期。

use contracts::joint::config_budget as budget;
use contracts::joint::protocol_faults::FaultCode;

use crate::motor::config_policy as policy;

/// 被监督的任务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TaskId {
    /// 20 kHz 电流环（ADC 完成事件）。
    Current = 0,
    /// CAN 任务（命令接收与状态发送）。
    Can = 1,
}

impl TaskId {
    /// 全部任务，顺序固定，保证多个任务同时停摆时判定可复现。
    pub const ALL: [Self; 2] = [Self::Current, Self::Can];
}

/// 监督者锁存的故障。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorFault {
    /// 超过 `budget::CAN_TIMEOUT_MS` 没有收到命令帧。
    CommTimeout,
    /// 某任务超过 [`policy::SUPERVISOR_TASK_STALL_US`] 没有心跳。
    TaskStalled(TaskId),
}

impl SupervisorFault {
    /// 映射到线上故障码（`contracts::joint::protocol_faults::FaultCode`）。
    ///
    /// 电流环停摆等价于 ADC/注入组没有完成，CAN 停摆等价于通信中断；不新增线协议码。
    pub const fn fault_code(self) -> FaultCode {
        match self {
            Self::CommTimeout | Self::TaskStalled(TaskId::Can) => FaultCode::CommTimeout,
            Self::TaskStalled(TaskId::Current) => FaultCode::AdcTimeout,
        }
    }
}

/// 一次监督轮询的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorEvent {
    /// 所有窗口内都有证据。
    Healthy,
    /// 首次发现通信超时。
    CommTimeout,
    /// 首次发现任务停摆。
    TaskStalled(TaskId),
    /// 故障已锁存，本次轮询不重复产生首次事件。
    Latched(SupervisorFault),
}

/// 监督者状态：每个任务最后心跳、最后命令帧与锁存原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Supervisor {
    last_current_us: u32,
    last_can_us: u32,
    last_cmd_us: u32,
    latched: Option<SupervisorFault>,
}

impl Supervisor {
    /// 以 `now_us` 作为所有窗口的起点。
    pub const fn new(now_us: u32) -> Self {
        Self {
            last_current_us: now_us,
            last_can_us: now_us,
            last_cmd_us: now_us,
            latched: None,
        }
    }

    /// 任务心跳。
    pub fn beat(&mut self, task: TaskId, now_us: u32) {
        match task {
            TaskId::Current => self.last_current_us = now_us,
            TaskId::Can => self.last_can_us = now_us,
        }
    }

    const fn last_beat_us(&self, task: TaskId) -> u32 {
        match task {
            TaskId::Current => self.last_current_us,
            TaskId::Can => self.last_can_us,
        }
    }

    /// 收到一帧有效命令。
    pub fn can_frame(&mut self, now_us: u32) {
        self.last_cmd_us = now_us;
    }

    /// 轮询：先查任务心跳，再查通信超时；命中即锁存。
    ///
    /// `require_comm` 为 `false` 时不检查命令超时（例如待机/未使能），但任务心跳始终检查。
    /// 超窗判据是严格大于：恰好等于窗口仍算健康。
    pub fn poll(&mut self, now_us: u32, require_comm: bool) -> SupervisorEvent {
        if let Some(fault) = self.latched {
            return SupervisorEvent::Latched(fault);
        }

        for task in TaskId::ALL {
            if now_us.wrapping_sub(self.last_beat_us(task)) > policy::SUPERVISOR_TASK_STALL_US {
                return self.latch(SupervisorFault::TaskStalled(task));
            }
        }

        let comm_timeout_us = u32::from(budget::CAN_TIMEOUT_MS) * 1_000;
        if require_comm && now_us.wrapping_sub(self.last_cmd_us) > comm_timeout_us {
            return self.latch(SupervisorFault::CommTimeout);
        }

        SupervisorEvent::Healthy
    }

    /// 显式清除锁存，并以 `now_us` 重新开始所有窗口。
    pub fn clear(&mut self, now_us: u32) {
        self.last_current_us = now_us;
        self.last_can_us = now_us;
        self.last_cmd_us = now_us;
        self.latched = None;
    }

    /// 是否已锁存故障。
    pub const fn is_latched(&self) -> bool {
        self.latched.is_some()
    }

    /// 锁存原因。
    pub const fn latched_fault(&self) -> Option<SupervisorFault> {
        self.latched
    }

    fn latch(&mut self, fault: SupervisorFault) -> SupervisorEvent {
        self.latched = Some(fault);
        match fault {
            SupervisorFault::CommTimeout => SupervisorEvent::CommTimeout,
            SupervisorFault::TaskStalled(task) => SupervisorEvent::TaskStalled(task),
        }
    }
}

#[cfg(test)]
mod tests;
