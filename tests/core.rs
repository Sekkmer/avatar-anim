use avatar_anim::{
    Animation, DuplicateKeyStrategy, JointData, PositionKey, RotationKey, SkeletonDefinition,
};
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

#[test]
fn tail_reset_uses_skeleton_local_positions_only() {
    let tail_bones = ["mTail1", "mTail2", "mTail3", "mTail4", "mTail5", "mTail6"];
    let xml = br#"
        <linden_skeleton>
            <bone name="mPelvis" pos="0.000 0.000 1.067">
                <bone name="mTail1" group="Tail" pos="-0.116 0.000 0.047"/>
                <bone name="mTail2" group="Tail" pos="-0.197 0.000 0.000"/>
                <bone name="mTail3" group="Tail" pos="-0.168 0.000 0.000"/>
                <bone name="mTail4" group="Tail" pos="-0.142 0.000 0.000"/>
                <bone name="mTail5" group="Tail" pos="-0.112 0.000 0.000"/>
                <bone name="mTail6" group="Tail" pos="-0.094 0.000 0.000"/>
            </bone>
        </linden_skeleton>
    "#;

    let skeleton = SkeletonDefinition::from_xml_reader(Cursor::new(xml)).unwrap();
    let anim = Animation::position_reset_from_skeleton(&skeleton, tail_bones, 6).unwrap();

    assert_eq!(anim.header.base_priority, 6);
    assert_eq!(anim.joints.len(), tail_bones.len());
    for (joint, name) in anim.joints.iter().zip(tail_bones) {
        assert_eq!(joint.name, name);
        assert_eq!(joint.priority, 6);
        assert!(joint.rotation_keys.is_empty());
        assert_eq!(joint.position_keys.len(), 2);
        assert_eq!(joint.position_keys[0].time, 0);
        assert_eq!(joint.position_keys[1].time, u16::MAX);
    }

    let tail1 = anim.joint("mTail1").unwrap();
    assert!((tail1.position_keys[0].pos - Vec3::new(-0.116, 0.0, 0.047)).length() < 1e-6);

    let skeleton_tail1 = skeleton.bone("mTail1").unwrap();
    assert_eq!(skeleton_tail1.parent.as_deref(), Some("mPelvis"));
    assert_eq!(
        skeleton_tail1.attributes.get("group").map(String::as_str),
        Some("Tail")
    );
    assert_eq!(skeleton.bones_with_prefix("mTail").len(), 6);
    assert_eq!(skeleton.bones_in_group("Tail").len(), 6);
}

#[test]
fn add_skeleton_positions_converts_deltas_to_local_positions() {
    let xml = br#"
        <linden_skeleton>
            <bone name="mTail1" pos="-0.116 0.000 0.047"/>
        </linden_skeleton>
    "#;
    let skeleton = SkeletonDefinition::from_xml_reader(Cursor::new(xml)).unwrap();
    let mut anim = Animation::default();
    anim.joints.push(JointData {
        name: "mTail1".into(),
        priority: 6,
        rotation_keys: vec![],
        position_keys: vec![PositionKey {
            time: u16::MAX,
            pos: Vec3::new(0.010, 0.020, -0.030),
        }],
    });

    anim.add_skeleton_positions(&skeleton).unwrap();

    let pos = anim.joint("mTail1").unwrap().position_keys[0].pos;
    assert!((pos - Vec3::new(-0.106, 0.020, 0.017)).length() < 1e-6);
}

#[test]
fn zero_position_deltas_can_be_dropped_before_adding_skeleton() {
    let xml = br#"
        <linden_skeleton>
            <bone name="mTail1" pos="-0.116 0.000 0.047"/>
            <bone name="mTail2" pos="-0.197 0.000 0.000"/>
        </linden_skeleton>
    "#;
    let skeleton = SkeletonDefinition::from_xml_reader(Cursor::new(xml)).unwrap();
    let mut anim = Animation::default();
    anim.joints.push(JointData {
        name: "mTail1".into(),
        priority: 6,
        rotation_keys: vec![],
        position_keys: vec![PositionKey {
            time: u16::MAX,
            pos: Vec3::ZERO,
        }],
    });
    anim.joints.push(JointData {
        name: "mTail2".into(),
        priority: 6,
        rotation_keys: vec![],
        position_keys: vec![PositionKey {
            time: u16::MAX,
            pos: Vec3::new(0.010, 0.0, 0.0),
        }],
    });

    anim.drop_zero_position_keys(1e-6)
        .drop_empty_joints()
        .add_skeleton_positions(&skeleton)
        .unwrap();

    assert!(anim.joint("mTail1").is_none());
    let pos = anim.joint("mTail2").unwrap().position_keys[0].pos;
    assert!((pos - Vec3::new(-0.187, 0.0, 0.0)).length() < 1e-6);
}

#[test]
fn set_duration_updates_loop_bounds() {
    let mut anim = Animation::default();
    anim.set_duration(2.5);
    assert_eq!(anim.header.duration, 2.5);
    assert_eq!(anim.header.loop_in_point, 0.0);
    assert_eq!(anim.header.loop_out_point, 2.5);
}
