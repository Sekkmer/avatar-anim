use avatar_anim::{
    Animation, JointData, PositionKey, RotationKey, SkeletonDefinition,
    bvh::{BvhDocument, BvhImportOptions},
};
use glam::{EulerRot, Quat, Vec3};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditorMode {
    Pose,
    Animation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Projection {
    Front,
    Side,
    Orbit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel {
    Rotation,
    Position,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    Blank,
    Firestorm,
    Bvh,
    Animation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionPolicy {
    None,
    NonZero,
    All,
}

#[derive(Clone, Debug)]
pub struct EditorDocument {
    pub name: String,
    pub animation: Animation,
    pub source: SourceKind,
    pub mode: EditorMode,
    pub position_policy: PositionPolicy,
    pub add_base_positions: bool,
    pub selected_joint: Option<String>,
    pub current_time: u16,
    pub priority: i32,
}

impl EditorDocument {
    pub fn blank() -> Self {
        Self {
            name: "untitled-pose.xml".to_owned(),
            animation: Animation::default(),
            source: SourceKind::Blank,
            mode: EditorMode::Pose,
            position_policy: PositionPolicy::None,
            add_base_positions: true,
            selected_joint: Some("mPelvis".to_owned()),
            current_time: u16::MAX,
            priority: 4,
        }
    }

    pub fn from_file(
        name: String,
        bytes: &[u8],
        skeleton: &SkeletonDefinition,
    ) -> Result<Self, String> {
        let lowercase = name.to_ascii_lowercase();
        if lowercase.ends_with(".xml") {
            let mut animation = Animation::from_llsd_xml(bytes, true)
                .map_err(|error| format!("Could not read Firestorm pose: {error}"))?;
            animation.drop_empty_joints();
            let selected_joint = animation.joints.first().map(|joint| joint.name.clone());
            Ok(Self {
                name,
                animation,
                source: SourceKind::Firestorm,
                mode: EditorMode::Pose,
                position_policy: PositionPolicy::None,
                add_base_positions: true,
                selected_joint,
                current_time: u16::MAX,
                priority: 4,
            })
        } else if lowercase.ends_with(".anim") {
            let mut animation = Animation::from_bytes(bytes)
                .map_err(|error| format!("Could not read animation: {error}"))?;
            normalize_positions_to_deltas(&mut animation, skeleton)?;
            let selected_joint = animation.joints.first().map(|joint| joint.name.clone());
            let priority = animation.header.base_priority.clamp(0, 7);
            Ok(Self {
                name,
                animation,
                source: SourceKind::Animation,
                mode: EditorMode::Animation,
                position_policy: PositionPolicy::All,
                add_base_positions: true,
                selected_joint,
                current_time: 0,
                priority,
            })
        } else if lowercase.ends_with(".bvh") {
            let bvh = BvhDocument::parse(bytes)
                .map_err(|error| format!("Could not read BVH animation: {error}"))?;
            let is_pose = bvh.frames.len() == 2;
            let mut animation = bvh
                .to_animation(
                    skeleton,
                    &BvhImportOptions {
                        priority: 4,
                        looped: is_pose,
                        ..BvhImportOptions::default()
                    },
                )
                .map_err(|error| format!("Could not convert BVH animation: {error}"))?;
            normalize_positions_to_deltas(&mut animation, skeleton)?;
            let selected_joint = animation.joints.first().map(|joint| joint.name.clone());
            Ok(Self {
                name,
                animation,
                source: SourceKind::Bvh,
                mode: if is_pose {
                    EditorMode::Pose
                } else {
                    EditorMode::Animation
                },
                position_policy: PositionPolicy::All,
                add_base_positions: true,
                selected_joint,
                current_time: if is_pose { u16::MAX } else { 0 },
                priority: 4,
            })
        } else {
            Err("Choose a Firestorm .xml/.bvh pose or a Second Life .anim file".to_owned())
        }
    }

    pub fn output_name(&self) -> String {
        let stem = self
            .name
            .rsplit_once('.')
            .map_or(self.name.as_str(), |(stem, _)| stem);
        format!("{stem}.anim")
    }

    pub fn export(&self, skeleton: &SkeletonDefinition) -> Result<Vec<u8>, String> {
        let mut animation = self.animation.clone();
        animation.set_priority(self.priority.clamp(0, 7));
        match self.position_policy {
            PositionPolicy::None => {
                animation.drop_position_keys();
            }
            PositionPolicy::NonZero => {
                animation.drop_zero_position_keys(1.0e-6);
            }
            PositionPolicy::All => {}
        }
        if self.position_policy != PositionPolicy::None && self.add_base_positions {
            animation
                .drop_empty_joints()
                .add_skeleton_positions(skeleton)
                .map_err(|error| error.to_string())?;
        }
        animation.drop_empty_joints().cleanup_keys();
        animation.to_bytes().map_err(|error| error.to_string())
    }

    pub fn joint_names(&self, skeleton: &SkeletonDefinition) -> Vec<String> {
        let mut names: BTreeSet<String> = skeleton
            .bones
            .iter()
            .map(|bone| bone.name.clone())
            .collect();
        names.extend(self.animation.joints.iter().map(|joint| joint.name.clone()));
        names.into_iter().collect()
    }

    pub fn joint_is_active(&self, name: &str) -> bool {
        self.animation
            .joint(name)
            .is_some_and(|joint| !joint.rotation_keys.is_empty() || !joint.position_keys.is_empty())
    }

    pub fn ensure_joint(&mut self, name: &str) -> &mut JointData {
        if self.animation.joint(name).is_none() {
            self.animation.joints.push(JointData {
                name: name.to_owned(),
                priority: self.priority,
                ..JointData::default()
            });
        }
        self.animation.joint_mut(name).expect("joint was inserted")
    }

    pub fn remove_joint(&mut self, name: &str) {
        self.animation.joints.retain(|joint| joint.name != name);
    }

    pub fn toggle_channel(&mut self, name: &str, channel: Channel, enabled: bool) {
        if enabled {
            let time = self.edit_time();
            let joint = self.ensure_joint(name);
            match channel {
                Channel::Rotation if joint.rotation_keys.is_empty() => {
                    joint.rotation_keys.push(RotationKey {
                        time,
                        rot: Quat::IDENTITY,
                    });
                }
                Channel::Position if joint.position_keys.is_empty() => {
                    joint.position_keys.push(PositionKey {
                        time,
                        pos: Vec3::ZERO,
                    });
                }
                _ => {}
            }
        } else if let Some(joint) = self.animation.joint_mut(name) {
            match channel {
                Channel::Rotation => joint.rotation_keys.clear(),
                Channel::Position => joint.position_keys.clear(),
            }
        }
        self.animation
            .joints
            .retain(|joint| !joint.rotation_keys.is_empty() || !joint.position_keys.is_empty());
    }

    pub fn channel_enabled(&self, name: &str, channel: Channel) -> bool {
        self.animation
            .joint(name)
            .is_some_and(|joint| match channel {
                Channel::Rotation => !joint.rotation_keys.is_empty(),
                Channel::Position => !joint.position_keys.is_empty(),
            })
    }

    pub fn edit_time(&self) -> u16 {
        match self.mode {
            EditorMode::Pose => u16::MAX,
            EditorMode::Animation => self.current_time,
        }
    }

    pub fn rotation_degrees(&self, name: &str) -> Vec3 {
        let rotation = self
            .animation
            .joint(name)
            .and_then(|joint| sample_rotation(&joint.rotation_keys, self.current_time))
            .unwrap_or(Quat::IDENTITY);
        let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }

    pub fn position(&self, name: &str) -> Vec3 {
        self.animation
            .joint(name)
            .and_then(|joint| sample_position(&joint.position_keys, self.current_time))
            .unwrap_or(Vec3::ZERO)
    }

    pub fn set_rotation_component(&mut self, name: &str, component: usize, degrees: f32) {
        let mut euler = self.rotation_degrees(name);
        euler[component] = degrees;
        let value = Quat::from_euler(
            EulerRot::XYZ,
            euler.x.to_radians(),
            euler.y.to_radians(),
            euler.z.to_radians(),
        )
        .normalize();
        let time = self.edit_time();
        let joint = self.ensure_joint(name);
        upsert_rotation(&mut joint.rotation_keys, time, value);
    }

    pub fn set_position_component(&mut self, name: &str, component: usize, value: f32) {
        let mut position = self.position(name);
        position[component] = value;
        let time = self.edit_time();
        let joint = self.ensure_joint(name);
        upsert_position(&mut joint.position_keys, time, position);
    }

    pub fn key_count(&self) -> usize {
        self.animation
            .joints
            .iter()
            .map(|joint| joint.rotation_keys.len() + joint.position_keys.len())
            .sum()
    }

    pub fn time_seconds(&self) -> f32 {
        self.current_time as f32 / u16::MAX as f32 * self.animation.header.duration
    }
}

fn normalize_positions_to_deltas(
    animation: &mut Animation,
    skeleton: &SkeletonDefinition,
) -> Result<(), String> {
    for joint in &mut animation.joints {
        if joint.position_keys.is_empty() {
            continue;
        }
        let base = skeleton.position(&joint.name).ok_or_else(|| {
            format!(
                "Could not convert position keys for unknown joint {} to skeleton deltas",
                joint.name
            )
        })?;
        for key in &mut joint.position_keys {
            key.pos -= base;
        }
    }
    Ok(())
}

pub fn sample_rotation(keys: &[RotationKey], time: u16) -> Option<Quat> {
    let first = keys.first()?;
    if time <= first.time {
        return Some(first.rot);
    }
    for pair in keys.windows(2) {
        if time <= pair[1].time {
            let span = pair[1].time.saturating_sub(pair[0].time);
            let amount = if span == 0 {
                1.0
            } else {
                time.saturating_sub(pair[0].time) as f32 / span as f32
            };
            return Some(pair[0].rot.slerp(pair[1].rot, amount));
        }
    }
    keys.last().map(|key| key.rot)
}

pub fn sample_position(keys: &[PositionKey], time: u16) -> Option<Vec3> {
    let first = keys.first()?;
    if time <= first.time {
        return Some(first.pos);
    }
    for pair in keys.windows(2) {
        if time <= pair[1].time {
            let span = pair[1].time.saturating_sub(pair[0].time);
            let amount = if span == 0 {
                1.0
            } else {
                time.saturating_sub(pair[0].time) as f32 / span as f32
            };
            return Some(pair[0].pos.lerp(pair[1].pos, amount));
        }
    }
    keys.last().map(|key| key.pos)
}

fn upsert_rotation(keys: &mut Vec<RotationKey>, time: u16, value: Quat) {
    if let Some(key) = keys.iter_mut().find(|key| key.time == time) {
        key.rot = value;
    } else {
        keys.push(RotationKey { time, rot: value });
        keys.sort_by_key(|key| key.time);
    }
}

fn upsert_position(keys: &mut Vec<PositionKey>, time: u16, value: Vec3) {
    if let Some(key) = keys.iter_mut().find(|key| key.time == time) {
        key.pos = value;
    } else {
        keys.push(PositionKey { time, pos: value });
        keys.sort_by_key(|key| key.time);
    }
}

#[derive(Clone, Debug)]
pub struct BonePose {
    pub name: String,
    pub parent: Option<String>,
    pub world_position: Vec3,
    pub active: bool,
}

pub fn pose_skeleton(document: &EditorDocument, skeleton: &SkeletonDefinition) -> Vec<BonePose> {
    let mut transforms = HashMap::<String, (Vec3, Quat)>::new();
    let mut poses = Vec::with_capacity(skeleton.bones.len());
    for bone in &skeleton.bones {
        let rotation = document
            .animation
            .joint(&bone.name)
            .and_then(|joint| sample_rotation(&joint.rotation_keys, document.current_time))
            .unwrap_or(Quat::IDENTITY);
        let keyed_position = document
            .animation
            .joint(&bone.name)
            .and_then(|joint| sample_position(&joint.position_keys, document.current_time));
        let local_position = keyed_position.map_or(bone.pos, |delta| bone.pos + delta);
        let (world_position, world_rotation) = bone
            .parent
            .as_ref()
            .and_then(|parent| transforms.get(parent))
            .map_or(
                (local_position, rotation),
                |(parent_position, parent_rotation)| {
                    (
                        *parent_position + *parent_rotation * local_position,
                        *parent_rotation * rotation,
                    )
                },
            );
        transforms.insert(bone.name.clone(), (world_position, world_rotation));
        let active = document.joint_is_active(&bone.name);
        let is_collision_volume = bone
            .attributes
            .get("group")
            .is_some_and(|group| group == "Collision");
        if !is_collision_volume || active {
            poses.push(BonePose {
                name: bone.name.clone(),
                parent: bone.parent.clone(),
                world_position,
                active,
            });
        }
    }
    poses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firestorm_pose_defaults_to_safe_pose_mode() {
        let xml = br#"<llsd><map><key>mHead</key><map>
            <key>enabled</key><boolean>true</boolean>
            <key>position</key><array><real>0</real><real>0</real><real>0</real></array>
            <key>rotation</key><array><real>0.1</real><real>0.2</real><real>0.3</real></array>
        </map></map></llsd>"#;

        let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
        let document = EditorDocument::from_file("pose.xml".to_owned(), xml, &skeleton).unwrap();
        assert_eq!(document.mode, EditorMode::Pose);
        assert_eq!(document.position_policy, PositionPolicy::None);
        assert!(document.add_base_positions);
        assert_eq!(document.animation.joints.len(), 1);
    }

    #[test]
    fn editing_at_playhead_inserts_a_key() {
        let mut document = EditorDocument::blank();
        document.mode = EditorMode::Animation;
        document.current_time = 12_345;
        document.set_rotation_component("mHead", 2, 30.0);

        let joint = document.animation.joint("mHead").unwrap();
        assert_eq!(joint.rotation_keys.len(), 1);
        assert_eq!(joint.rotation_keys[0].time, 12_345);
        assert!((document.rotation_degrees("mHead").z - 30.0).abs() < 0.01);
    }

    #[test]
    fn pose_export_converts_position_delta_with_embedded_skeleton() {
        let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
        let mut document = EditorDocument::blank();
        document.position_policy = PositionPolicy::NonZero;
        document.toggle_channel("mTail1", Channel::Position, true);
        document.set_position_component("mTail1", 0, 0.01);

        let bytes = document.export(&skeleton).unwrap();
        let animation = Animation::from_bytes(&bytes).unwrap();
        let output = animation.joint("mTail1").unwrap().position_keys[0].pos;
        let expected = skeleton.position("mTail1").unwrap() + Vec3::X * 0.01;
        assert!((output - expected).length() < 5.0e-4);
    }

    #[test]
    fn position_policy_controls_zero_delta_export() {
        let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
        let mut document = EditorDocument::blank();
        document.toggle_channel("mTail1", Channel::Position, true);

        let without_positions =
            Animation::from_bytes(&document.export(&skeleton).unwrap()).unwrap();
        assert!(without_positions.joint("mTail1").is_none());

        document.position_policy = PositionPolicy::NonZero;
        let without_zero_deltas =
            Animation::from_bytes(&document.export(&skeleton).unwrap()).unwrap();
        assert!(without_zero_deltas.joint("mTail1").is_none());

        document.position_policy = PositionPolicy::All;
        let with_all_deltas = Animation::from_bytes(&document.export(&skeleton).unwrap()).unwrap();
        let output = with_all_deltas.joint("mTail1").unwrap().position_keys[0].pos;
        assert!((output - skeleton.position("mTail1").unwrap()).length() < 5.0e-4);
    }

    #[test]
    fn anim_import_normalizes_local_positions_to_deltas() {
        let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
        let base = skeleton.position("mTail1").unwrap();
        let mut animation = Animation::default();
        animation.joints.push(JointData {
            name: "mTail1".to_owned(),
            position_keys: vec![PositionKey {
                time: 0,
                pos: base + Vec3::X * 0.02,
            }],
            ..JointData::default()
        });
        let bytes = animation.to_bytes().unwrap();

        let document =
            EditorDocument::from_file("tail.anim".to_owned(), &bytes, &skeleton).unwrap();

        assert_eq!(document.position_policy, PositionPolicy::All);
        assert!((document.position("mTail1") - Vec3::X * 0.02).length() < 5.0e-4);
    }

    #[test]
    fn two_frame_bvh_opens_as_a_pose() {
        let skeleton = SkeletonDefinition::embedded_avatar().unwrap();
        let bvh = b"HIERARCHY ROOT mPelvis { OFFSET 0 0 0 CHANNELS 6 Xposition Yposition Zposition Zrotation Xrotation Yrotation JOINT mTail1 { OFFSET 0 0 0 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET 0 0 1 } } } MOTION Frames: 2 Frame Time: 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 10 0";

        let document = EditorDocument::from_file("tail.bvh".to_owned(), bvh, &skeleton).unwrap();

        assert_eq!(document.source, SourceKind::Bvh);
        assert_eq!(document.mode, EditorMode::Pose);
        assert_eq!(document.position_policy, PositionPolicy::All);
        assert!(document.animation.header.looped != 0);
        assert!((document.rotation_degrees("mTail1").y - 10.0).abs() < 0.01);
    }
}
