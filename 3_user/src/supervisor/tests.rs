use super::{Supervisor, SupervisorEvent, SupervisorFault, TaskId};
use contracts::faults::FaultCode;
use contracts::limits::budget;

fn beat_all(supervisor: &mut Supervisor, now_us: u32) {
    supervisor.beat(TaskId::Current, now_us);
    supervisor.beat(TaskId::Can, now_us);
}

fn armed_at(now_us: u32) -> Supervisor {
    let mut supervisor = Supervisor::new(now_us);
    beat_all(&mut supervisor, now_us);
    supervisor.can_frame(now_us);
    supervisor
}

#[test]
fn healthy_inside_all_windows() {
    let mut supervisor = armed_at(0);
    beat_all(&mut supervisor, 19_999);
    assert_eq!(supervisor.poll(19_999, true), SupervisorEvent::Healthy);
}

#[test]
fn comm_timeout_is_strict_at_the_boundary_and_latches() {
    let mut supervisor = armed_at(0);
    let limit_us = u32::from(budget::CAN_TIMEOUT_MS) * 1_000;

    beat_all(&mut supervisor, limit_us);
    assert_eq!(supervisor.poll(limit_us, true), SupervisorEvent::Healthy);

    beat_all(&mut supervisor, limit_us + 1);
    assert_eq!(
        supervisor.poll(limit_us + 1, true),
        SupervisorEvent::CommTimeout
    );
    beat_all(&mut supervisor, limit_us + 2);
    assert_eq!(
        supervisor.poll(limit_us + 2, true),
        SupervisorEvent::Latched(SupervisorFault::CommTimeout)
    );

    // 新帧不能自动解除锁存。
    supervisor.can_frame(limit_us + 3);
    beat_all(&mut supervisor, limit_us + 3);
    assert_eq!(
        supervisor.poll(limit_us + 3, true),
        SupervisorEvent::Latched(SupervisorFault::CommTimeout)
    );

    supervisor.clear(limit_us + 4);
    assert_eq!(supervisor.poll(limit_us + 4, true), SupervisorEvent::Healthy);
}

#[test]
fn current_task_stall_wins_when_both_are_late() {
    let mut supervisor = Supervisor::new(0);
    supervisor.can_frame(0);
    let late_us = 1_001;
    assert_eq!(
        supervisor.poll(late_us, false),
        SupervisorEvent::TaskStalled(TaskId::Current)
    );
    assert_eq!(
        supervisor.latched_fault(),
        Some(SupervisorFault::TaskStalled(TaskId::Current))
    );
    assert_eq!(
        SupervisorFault::TaskStalled(TaskId::Current).fault_code(),
        FaultCode::AdcTimeout
    );
}

#[test]
fn can_task_stall_is_detected_independently() {
    let mut supervisor = Supervisor::new(0);
    supervisor.can_frame(0);
    supervisor.beat(TaskId::Current, 1_001);
    assert_eq!(
        supervisor.poll(1_001, false),
        SupervisorEvent::TaskStalled(TaskId::Can)
    );
}

#[test]
fn disabled_comm_check_still_supervises_tasks() {
    let mut supervisor = supervisor_with_fresh_comms(19_999);
    assert_eq!(supervisor.poll(19_999, false), SupervisorEvent::Healthy);

    // 任务心跳停住：即使关掉通信检查，仍要抓当前任务停摆。
    assert_eq!(
        supervisor.poll(21_000, false),
        SupervisorEvent::TaskStalled(TaskId::Current)
    );
}

fn supervisor_with_fresh_comms(now_us: u32) -> Supervisor {
    let mut supervisor = Supervisor::new(now_us);
    supervisor.can_frame(now_us);
    beat_all(&mut supervisor, now_us);
    supervisor
}

#[test]
fn clear_rearms_all_windows() {
    let mut supervisor = armed_at(0);
    beat_all(&mut supervisor, 20_001);
    assert_eq!(supervisor.poll(20_001, true), SupervisorEvent::CommTimeout);

    supervisor.clear(20_001);
    assert!(!supervisor.is_latched());
    assert_eq!(supervisor.poll(20_001, true), SupervisorEvent::Healthy);

    beat_all(&mut supervisor, 40_001);
    assert_eq!(supervisor.poll(40_001, true), SupervisorEvent::Healthy);
    beat_all(&mut supervisor, 40_002);
    assert_eq!(
        supervisor.poll(40_002, true),
        SupervisorEvent::CommTimeout
    );
}

#[test]
fn fault_code_mapping_does_not_invent_wire_codes() {
    assert_eq!(
        SupervisorFault::CommTimeout.fault_code(),
        FaultCode::CommTimeout
    );
    assert_eq!(
        SupervisorFault::TaskStalled(TaskId::Can).fault_code(),
        FaultCode::CommTimeout
    );
    assert_eq!(
        SupervisorFault::TaskStalled(TaskId::Current).fault_code(),
        FaultCode::AdcTimeout
    );
}
