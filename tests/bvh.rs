use avatar_anim::bvh::{BvhDocument, BvhImportOptions, UnknownJointPolicy};
use avatar_anim::{AnimError, Animation, SkeletonDefinition};
use glam::{Quat, Vec3};

const FIRESTORM_POSE: &str = r#"
HIERARCHY
ROOT hip
{
    OFFSET 0 0 0
    CHANNELS 6 Xposition Yposition Zposition Zrotation Xrotation Yrotation
    JOINT mTail1
    {
        OFFSET 0 0 0
        CHANNELS 3 Zrotation Xrotation Yrotation
        End Site { OFFSET 0 0 0.1 }
    }
}
MOTION
Frames: 2
Frame Time: 1
0 0 0 0 0 0  0 0 0
0 3.149606 0 0 0 0  0.003026 2 -0.001640
"#;

#[test]
fn parses_firestorm_hierarchy_and_motion() {
    let document = BvhDocument::parse(FIRESTORM_POSE.as_bytes()).unwrap();
    assert_eq!(document.joints.len(), 2);
    assert_eq!(document.joints[0].name, "hip");
    assert_eq!(document.joints[0].children, vec![1]);
    assert_eq!(document.joints[1].parent, Some(0));
    assert_eq!(document.joints[1].end_site, Some(Vec3::new(0.0, 0.0, 0.1)));
    assert_eq!(document.total_channels, 9);
    assert_eq!(document.frames.len(), 2);
    assert_eq!(document.joint_values(1, 1).unwrap().len(), 3);
}

#[test]
fn conversion_matches_firestorm_axis_and_reference_frame_rules() {
    let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
    let animation = Animation::from_bvh(
        FIRESTORM_POSE.as_bytes(),
        &skeleton,
        &BvhImportOptions::default(),
    )
    .unwrap();

    assert_eq!(animation.header.duration, 1.0);
    let pelvis = animation.joint("mPelvis").unwrap();
    assert!((pelvis.position_keys[0].pos - Vec3::Z * 0.08).length() < 1.0e-5);
    assert_eq!(pelvis.position_keys[0].time, 0);

    let tail = animation.joint("mTail1").unwrap();
    let expected = Quat::from_euler(
        glam::EulerRot::XYZ,
        0.003026_f32.to_radians(),
        2.0_f32.to_radians(),
        -0.001640_f32.to_radians(),
    );
    assert!(tail.rotation_keys[0].rot.abs_diff_eq(expected, 1.0e-5));
}

#[test]
fn parses_bom_comments_crlf_and_spaced_colons() {
    let input = "\u{feff}HIERARCHY\r\nROOT mPelvis {\r\n# root\r\nOFFSET 0 0 0\r\nCHANNELS 3 Xrotation Yrotation Zrotation\r\nEnd Site { OFFSET 0 0 1 }\r\n}\r\nMOTION\r\nFrames : 2\r\nFrame Time : 0.5\r\n0 0 0 // reference\r\n1 2 3\r\n";
    let document = BvhDocument::parse(input.as_bytes()).unwrap();
    assert_eq!(document.frames.len(), 2);
    assert_eq!(document.frame_time, 0.5);
}

#[test]
fn accepts_firestorm_legacy_empty_offsets_as_zero() {
    let input = "HIERARCHY ROOT mPelvis { OFFSET CHANNELS 3 Xrotation Yrotation Zrotation End Site { OFFSET } } MOTION Frames: 2 Frame Time: 1 0 0 0 1 0 0";
    let document = BvhDocument::parse(input.as_bytes()).unwrap();
    assert_eq!(document.joints[0].offset, Vec3::ZERO);
    assert_eq!(document.joints[0].end_site, Some(Vec3::ZERO));
}

#[test]
fn unknown_joint_policy_can_preserve_ignore_or_reject() {
    let input = FIRESTORM_POSE.replace("mTail1", "MocapTail");
    let skeleton = SkeletonDefinition::embedded_avatar().unwrap();

    let preserved =
        Animation::from_bvh(input.as_bytes(), &skeleton, &BvhImportOptions::default()).unwrap();
    assert!(preserved.joint("MocapTail").is_some());

    let ignored = Animation::from_bvh(
        input.as_bytes(),
        &skeleton,
        &BvhImportOptions {
            unknown_joints: UnknownJointPolicy::Ignore,
            ..BvhImportOptions::default()
        },
    )
    .unwrap();
    assert!(ignored.joint("MocapTail").is_none());

    let error = Animation::from_bvh(
        input.as_bytes(),
        &skeleton,
        &BvhImportOptions {
            unknown_joints: UnknownJointPolicy::Error,
            ..BvhImportOptions::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("MocapTail"));
}

#[test]
fn malformed_motion_reports_source_location() {
    let input = FIRESTORM_POSE.replace("3.149606", "not-a-number");
    let error = BvhDocument::parse(input.as_bytes()).unwrap_err();
    match error {
        AnimError::BvhParse {
            line,
            column,
            message,
        } => {
            assert!(line > 10);
            assert!(column > 0);
            assert!(message.contains("not-a-number"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn single_frame_parses_but_is_not_an_sl_animation() {
    let input = FIRESTORM_POSE
        .replace("Frames: 2", "Frames: 1")
        .replace("0 3.149606 0 0 0 0  0.003026 2 -0.001640\n", "");
    let document = BvhDocument::parse(input.as_bytes()).unwrap();
    let error = document
        .to_animation(
            &SkeletonDefinition::embedded_avatar().unwrap(),
            &BvhImportOptions::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("reference frame"));
}

#[test]
fn supports_all_six_euler_channel_orders() {
    let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
    for order in ["XYZ", "XZY", "YXZ", "YZX", "ZXY", "ZYX"] {
        let channels = order
            .chars()
            .map(|axis| format!("{axis}rotation"))
            .collect::<Vec<_>>()
            .join(" ");
        let input = format!(
            "HIERARCHY ROOT mPelvis {{ OFFSET 0 0 0 CHANNELS 3 Xrotation Yrotation Zrotation JOINT MocapBone {{ OFFSET 0 0 0 CHANNELS 3 {channels} End Site {{ OFFSET 0 0 1 }} }} }} MOTION Frames: 2 Frame Time: 1 0 0 0 0 0 0 0 0 0 10 20 30"
        );
        let animation = Animation::from_bvh(
            input.as_bytes(),
            &skeleton,
            &BvhImportOptions {
                optimize: false,
                ..BvhImportOptions::default()
            },
        )
        .unwrap();
        let actual = animation.joint("MocapBone").unwrap().rotation_keys[0].rot;
        let expected = order.chars().zip([10.0_f32, 20.0, 30.0]).fold(
            Quat::IDENTITY,
            |rotation, (axis, degrees)| {
                rotation
                    * match axis {
                        'X' => Quat::from_rotation_x(degrees.to_radians()),
                        'Y' => Quat::from_rotation_y(degrees.to_radians()),
                        'Z' => Quat::from_rotation_z(degrees.to_radians()),
                        _ => unreachable!(),
                    }
            },
        );
        assert!(actual.abs_diff_eq(expected, 1.0e-6), "order {order}");
    }
}

#[test]
fn viewer_timing_omits_reference_frame_and_ends_at_last_frame() {
    let input = "HIERARCHY ROOT mPelvis { OFFSET 0 0 0 CHANNELS 3 Xrotation Yrotation Zrotation End Site { OFFSET 0 0 1 } } MOTION Frames: 4 Frame Time: 0.25 0 0 0 1 0 0 2 0 0 3 0 0";
    let animation = Animation::from_bvh(
        input.as_bytes(),
        &SkeletonDefinition::embedded_avatar().unwrap(),
        &BvhImportOptions {
            optimize: false,
            ..BvhImportOptions::default()
        },
    )
    .unwrap();
    assert_eq!(animation.header.duration, 0.5);
    let times = animation
        .joint("mPelvis")
        .unwrap()
        .rotation_keys
        .iter()
        .map(|key| key.time)
        .collect::<Vec<_>>();
    assert_eq!(times, vec![0, 32_768, u16::MAX]);
}
