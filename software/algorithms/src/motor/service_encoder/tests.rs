//! MT6701 帧解析与接受判据机的 host 测试。

use super::frame::crc6;
use super::*;
use crate::motor::config_policy as policy;
use contracts::joint::config_budget as budget;
use contracts::motor::config_hw as hw;

/// 24 bit 字 → 三个 MSB 先出的字节（高 8 bit 恒为 0）。
fn bytes_of(word: u32) -> [u8; 3] {
    let [_, hi, mid, lo] = word.to_be_bytes();
    [hi, mid, lo]
}

/// 构造合法帧：14 bit 角 + 4 bit Mg + 6 bit CRC。
fn encode(angle_raw: u16, mag: u8) -> [u8; 3] {
    let payload = (u32::from(angle_raw) << hw::ENC_MAG_BITS) | u32::from(mag & 0x0F);
    let word = (payload << hw::ENC_CRC_BITS) | u32::from(crc6(payload));
    bytes_of(word)
}

/// 单个 CRC 位翻转的坏帧（payload 合法但帧尾错）。
fn broken_frame() -> [u8; 3] {
    bytes_of(0x00FA_781B ^ 1)
}

#[test]
fn known_frame_parses_to_frozen_fields() {
    // 一手手册样例帧：θ=16030、Mg=0、CRC=0x1B。
    let word = 0x00FA_781B;
    assert_eq!(crc6(word >> hw::ENC_CRC_BITS), 0x1B);

    let raw = bytes_of(word);
    let frame = match parse(raw) {
        Ok(frame) => frame,
        Err(_) => panic!("样例帧必须通过 CRC"),
    };
    assert_eq!(frame.angle_raw, 16_030);
    assert_eq!(frame.mag, 0);
    assert_eq!(frame.crc, 0x1B);
    assert_eq!(frame.field_code(), 0);
    assert!(!frame.is_locked());

    let expected = 16_030.0 * core::f32::consts::TAU / f32::from(hw::ENC_COUNTS_PER_REV);
    assert!((angle_rad(frame.angle_raw) - expected).abs() < 1e-6);
}

#[test]
fn all_zero_frame_is_a_legal_frame() {
    // 0x000000：θ=0、Mg=0、CRC=0，CRC 必须通过（盲区由判据机护栏负责）。
    assert_eq!(crc6(0), 0);
    let frame = match parse([0, 0, 0]) {
        Ok(frame) => frame,
        Err(_) => panic!("全 0 帧是合法帧"),
    };
    assert_eq!(frame.angle_raw, 0);
    assert_eq!(frame.mag, 0);
    assert_eq!(frame.crc, 0);
}

#[test]
fn every_single_bit_flip_is_rejected() {
    // 24 个位置逐一翻转（角度 14 bit、Mg 4 bit、CRC 6 bit 全覆盖）。
    let word = 0x00FA_781Bu32;
    let mut pos = 0;
    while pos < u32::from(hw::ENC_FRAME_BITS) {
        let flipped = word ^ (1u32 << pos);
        assert_eq!(parse(bytes_of(flipped)), Err(FrameError::Crc), "bit {pos}");
        pos += 1;
    }
}

#[test]
fn mag_lock_bit_is_a_hard_failure() {
    let raw = encode(16_030, 0b1000);
    let frame = match parse(raw) {
        Ok(frame) => frame,
        Err(_) => panic!("构造的帧必须通过 CRC"),
    };
    assert!(frame.is_locked());

    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(raw).health, Health::Rejected(Fault::MagLock));
    assert_eq!(monitor.update(raw).health, Health::Latched(Fault::MagLock));
    assert!(monitor.is_latched());
    assert_eq!(monitor.latched_fault(), Some(Fault::MagLock));
}

#[test]
fn zero_frame_after_nonzero_angle_fails_but_startup_zero_does_not() {
    // 冷启动就是 0：合法帧，连续 40 帧也不误报（此前没有非 0 角）。
    let mut startup = EncoderMonitor::new(EncoderParams::default());
    let mut i = 0;
    while i < 40 {
        let out = startup.update([0, 0, 0]);
        assert_eq!(out.health, Health::Ok, "startup zero {i}");
        assert_eq!(out.theta_rad, 0.0);
        i += 1;
    }
    assert!(!startup.is_latched());

    // 上一接受角非 0：全 0 帧进入硬失败（这里是跳变判据先抓；回绕护栏另有测试）。
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(16_030, 0)).health, Health::Ok);
    let first = monitor.update([0, 0, 0]);
    assert!(first.health.is_hard_failure());
    assert!(!first.health.is_latched());
    // 连续 2 帧锁存。
    assert!(monitor.update([0, 0, 0]).health.is_latched());
}

#[test]
fn zero_run_guard_catches_stuck_low_miso_near_wrap() {
    // 16380 → 0 是合法回绕（Δ=4），前 7 帧全 0 都能通过跳变判据；
    // 第 8 帧由全 0 计数抓出，这正是 MISO 恒低盲区的护栏。
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(16_380, 0)).health, Health::Ok);
    let mut i = 0;
    while i < 7 {
        assert_eq!(
            monitor.update([0, 0, 0]).health,
            Health::Ok,
            "wrap zero {i}"
        );
        i += 1;
    }
    assert_eq!(
        monitor.update([0, 0, 0]).health,
        Health::Rejected(Fault::ZeroFrame)
    );
    assert_eq!(
        monitor.update([0, 0, 0]).health,
        Health::Latched(Fault::ZeroFrame)
    );
}

#[test]
fn jump_over_limit_rejects_then_latches() {
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(1_000, 0)).health, Health::Ok);

    let jumped = encode(6_000, 0);
    let first = monitor.update(jumped);
    assert_eq!(first.health, Health::Rejected(Fault::Jump));
    assert!(!first.health.is_latched());
    // 失败时不更新基准：输出保持上一接受角，控制环不会看到假跳变。
    assert!((first.theta_rad - angle_rad(1_000)).abs() < 1e-6);

    assert_eq!(monitor.update(jumped).health, Health::Latched(Fault::Jump));
}

#[test]
fn wrap_around_pi_is_not_a_jump() {
    let mut forward = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(forward.update(encode(16_383, 0)).health, Health::Ok);
    assert_eq!(forward.update(encode(0, 0)).health, Health::Ok);

    let mut backward = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(backward.update(encode(0, 0)).health, Health::Ok);
    assert_eq!(backward.update(encode(16_383, 0)).health, Health::Ok);
}

#[test]
fn weak_field_is_reported_but_not_rejected_until_persistent() {
    // Mg[1:0] = 2（磁场过弱）：不拒帧，健康度上报；连续 20 帧升级为硬失败。
    let weak = encode(1_000, 0b0010);
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    let mut i = 0;
    while i < 19 {
        let out = monitor.update(weak);
        assert_eq!(out.health, Health::MagWarn { code: 2 }, "frame {i}");
        assert!((out.theta_rad - angle_rad(1_000)).abs() < 1e-6);
        i += 1;
    }
    assert_eq!(
        monitor.update(weak).health,
        Health::Rejected(Fault::MagStuck)
    );
    assert_eq!(
        monitor.update(weak).health,
        Health::Latched(Fault::MagStuck)
    );
}

#[test]
fn clean_frame_resets_magnetic_warning_streak() {
    let weak = encode(1_000, 0b0010);
    let clean = encode(1_000, 0);
    let mut monitor = EncoderMonitor::new(EncoderParams::default());

    let mut i = 0;
    while i < 10 {
        assert_eq!(
            monitor.update(weak).health,
            Health::MagWarn { code: 2 },
            "warm-up {i}"
        );
        i += 1;
    }
    assert_eq!(monitor.update(clean).health, Health::Ok);

    // 重置后的 19 帧告警仍在软告警区间（连续计数确实被清零）。
    let mut j = 0;
    while j < 19 {
        assert_eq!(
            monitor.update(weak).health,
            Health::MagWarn { code: 2 },
            "after reset {j}"
        );
        j += 1;
    }
    assert_eq!(
        monitor.update(weak).health,
        Health::Rejected(Fault::MagStuck)
    );
}

#[test]
fn jump_limit_matches_policy_mirror() {
    // `policy::B_MAX_LSB` 是同一算式的 const 手算镜像（f32 版本不能进 const），
    // 跳变盲区的断言依赖它，所以这里钉住两者相等。
    assert_eq!(
        jump_limit_lsb(policy::ENC_MECH_RPM_MAX, budget::ISR_PERIOD_US),
        policy::B_MAX_LSB
    );
    assert_eq!(EncoderParams::default().jump_limit_lsb, policy::B_MAX_LSB);
}

#[test]
fn single_crc_glitch_does_not_latch() {
    let b = EncoderParams::default().jump_limit_lsb;
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(1_000, 0)).health, Health::Ok);

    // 一帧 CRC 错：只算丢失，不锁存。
    assert_eq!(
        monitor.update(broken_frame()).health,
        Health::Rejected(Fault::Crc)
    );
    assert!(!monitor.is_latched());
    assert_eq!(monitor.missed_frames(), 1);

    // 下一帧位移落在 2B 内：接受，丢失计数归零（一次抖动不升级为停机）。
    let out = monitor.update(encode(1_000 + 2 * b - 1, 0));
    assert_eq!(out.health, Health::Ok);
    assert_eq!(monitor.missed_frames(), 0);
}

#[test]
fn allowance_grows_with_missed_frames() {
    let b = EncoderParams::default().jump_limit_lsb;
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(2_000, 0)).health, Health::Ok);

    // 盲推窗口 = MAX_BLIND_US / ISR_PERIOD_US = 1 拍：先丢一帧。
    let mut i = 0;
    while i < policy::ENC_MAX_MISS {
        assert_eq!(
            monitor.update(broken_frame()).health,
            Health::Rejected(Fault::Crc),
            "miss {i}"
        );
        i += 1;
    }
    assert_eq!(monitor.missed_frames(), policy::ENC_MAX_MISS);

    // missed = 1 ⇒ allow = 2B；2B 内的位移必须接受，而不是误判跳变。
    let out = monitor.update(encode(2_000 + 2 * b - 1, 0));
    assert_eq!(out.health, Health::Ok);
    assert_eq!(monitor.missed_frames(), 0);

    // 放宽是按丢失拍数线性给的，不会多给：同样一次丢失后位移 2B+1 必须拒，
    // 且这一次失败把 missed 推到 max_miss 之上，直接锁存。
    let mut strict = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(strict.update(encode(2_000, 0)).health, Health::Ok);
    assert_eq!(
        strict.update(broken_frame()).health,
        Health::Rejected(Fault::Crc)
    );
    let out = strict.update(encode(2_000 + 2 * b + 1, 0));
    assert_eq!(out.health, Health::Latched(Fault::Jump));
}

#[test]
fn too_many_missed_latches() {
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(3_000, 0)).health, Health::Ok);

    let mut i = 0;
    while i < policy::ENC_MAX_MISS {
        let out = monitor.update(broken_frame());
        assert_eq!(out.health, Health::Rejected(Fault::Crc), "miss {i}");
        assert!(!out.health.is_latched());
        i += 1;
    }
    // MAX_MISS + 1 帧：丢失窗口耗尽，直接锁存。
    assert_eq!(
        monitor.update(broken_frame()).health,
        Health::Latched(Fault::Crc)
    );
    assert_eq!(monitor.missed_frames(), policy::ENC_MAX_MISS + 1);
    // 锁存后即使来好帧也不接受（只能显式 reset）。
    assert_eq!(
        monitor.update(encode(3_000, 0)).health,
        Health::Latched(Fault::Crc)
    );
}

#[test]
fn jump_beyond_scaled_bound_latches() {
    let b = EncoderParams::default().jump_limit_lsb;
    let mut monitor = EncoderMonitor::new(EncoderParams::default());
    assert_eq!(monitor.update(encode(4_000, 0)).health, Health::Ok);

    // 造成 missed = 1 的语义跳变（#1，未锁存）。
    assert_eq!(
        monitor.update(encode(4_000 + 8 * b, 0)).health,
        Health::Rejected(Fault::Jump)
    );
    assert_eq!(monitor.missed_frames(), 1);

    // missed = 1 ⇒ allow = 2B；来帧位移 2.5B 仍超界 ⇒ 语义故障 #2 ⇒ 锁存。
    let displacement = b * 5 / 2;
    assert_eq!(
        monitor.update(encode(4_000 + displacement, 0)).health,
        Health::Latched(Fault::Jump)
    );
}

#[test]
fn crc6_table_matches_bitwise_over_all_payloads() {
    // 2^18 个载荷全跑：合法码字必须通过，翻 1 bit 必须被拒。
    for payload in 0..(1u32 << 18) {
        let crc = u32::from(crc6(payload));
        let word = (payload << hw::ENC_CRC_BITS) | crc;
        let [_, hi, mid, lo] = word.to_be_bytes();
        assert!(parse([hi, mid, lo]).is_ok(), "payload = {payload}");

        let flipped = word ^ (1 << 5);
        let [_, hi, mid, lo] = flipped.to_be_bytes();
        assert!(parse([hi, mid, lo]).is_err(), "payload = {payload}");
    }
}
