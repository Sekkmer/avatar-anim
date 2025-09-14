use avatar_anim::{Animation, DuplicateKeyStrategy, JointData, PositionKey, RotationKey};
use glam::{Quat, Vec3};
use std::io::Cursor;

#[test]
fn quaternion_roundtrip() {
    let quats = [
        Quat::IDENTITY,
        Quat::from_rotation_x(0.3),
        Quat::from_rotation_y(-0.7),
        Quat::from_rotation_z(1.1),
        Quat::from_euler(glam::EulerRot::XYZ, 0.5, -1.0, 0.25),
    ];
    for q in quats {
        let mut buf = Cursor::new(Vec::new());
        avatar_anim::io::write_rot_quat(&q, &mut buf, binrw::Endian::Little, ()).unwrap();
        buf.set_position(0);
        let qr = avatar_anim::io::read_rot_quat(&mut buf, binrw::Endian::Little, ()).unwrap();
        let dot = q.normalize().dot(qr);
        assert!(
            dot.abs() > 0.999,
            "Quaternion roundtrip accuracy too low: {} vs {} (dot={})",
            q,
            qr,
            dot
        );
    }
}

#[test]
fn position_roundtrip_quant_error_bound() {
    let v = Vec3::new(1.2345, -2.2222, 4.9999_f32.min(4.9999));
    let mut buf = Cursor::new(Vec::new());
    avatar_anim::io::write_pos_vec3(&v, &mut buf, binrw::Endian::Little, ()).unwrap();
    buf.set_position(0);
    let vr = avatar_anim::io::read_pos_vec3(&mut buf, binrw::Endian::Little, ()).unwrap();
    let err = (v - vr).length();
    assert!(
        err < 5e-4,
        "Position quantization error too large: {} vs {} (err={})",
        v,
        vr,
        err
    );
}

#[test]
fn duplicate_key_strategy_average() {
    let mut anim = Animation::default();
    anim.joints.push(JointData {
        name: "Spine".into(),
        priority: 6,
        rotation_keys: vec![
            RotationKey {
                time: 10,
                rot: Quat::from_rotation_x(0.2),
            },
            RotationKey {
                time: 10,
                rot: Quat::from_rotation_x(0.4),
            },
        ],
        position_keys: vec![
            PositionKey {
                time: 10,
                pos: Vec3::new(1.0, 0.0, 0.0),
            },
            PositionKey {
                time: 10,
                pos: Vec3::new(3.0, 0.0, 0.0),
            },
        ],
    });
    anim.cleanup_keys_with(DuplicateKeyStrategy::Average);
    let joint = anim.joint("Spine").unwrap();
    assert_eq!(joint.rotation_keys.len(), 1);
    assert_eq!(joint.position_keys.len(), 1);
    assert!((joint.position_keys[0].pos.x - 2.0).abs() < 1e-6);
}

#[test]
fn duplicate_key_strategy_keep_last() {
    let mut anim = Animation::default();
    anim.joints.push(JointData {
        name: "Head".into(),
        priority: 6,
        rotation_keys: vec![
            RotationKey {
                time: 5,
                rot: Quat::from_rotation_y(0.1),
            },
            RotationKey {
                time: 5,
                rot: Quat::from_rotation_y(0.5),
            },
        ],
        position_keys: vec![],
    });
    anim.cleanup_keys_with(DuplicateKeyStrategy::KeepLast);
    let joint = anim.joint("Head").unwrap();
    assert_eq!(joint.rotation_keys.len(), 1);
    let expected = Quat::from_rotation_y(0.5).normalize();
    let dot = expected.dot(joint.rotation_keys[0].rot);
    assert!(dot > 0.999, "Last key not preserved as expected");
}
