use binrw::binrw;
use glam::{EulerRot, Quat, Vec3};
use llsd_rs::Llsd;
use std::collections::HashSet;

mod io;

use crate::io::*;

#[binrw]
#[brw(little)]
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationHeader {
    pub version: u16,
    pub sub_version: u16,
    pub base_priority: i32,
    pub duration: f32,
    #[br(parse_with = read_null_terminated_string)]
    #[bw(write_with = write_null_terminated_string)]
    pub emote_name: String,
    pub loop_in_point: f32,
    pub loop_out_point: f32,
    pub looped: i32,
    pub ease_in_duration: f32,
    pub ease_out_duration: f32,
    pub hand_pose: u32,
}

impl Default for AnimationHeader {
    fn default() -> Self {
        Self {
            version: 1,
            sub_version: 0,
            base_priority: 6,
            duration: 0.017,
            emote_name: String::new(),
            loop_in_point: 0.0,
            loop_out_point: 0.017,
            looped: 1,
            ease_in_duration: 1.0,
            ease_out_duration: 1.0,
            hand_pose: 0,
        }
    }
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RotationKey {
    pub time: u16,
    #[br(parse_with = read_rot_quat)]
    #[bw(write_with = write_rot_quat)]
    pub rot: Quat,
}

impl From<Quat> for RotationKey {
    fn from(rot: Quat) -> Self {
        Self { time: 0, rot }
    }
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PositionKey {
    pub time: u16,
    #[br(parse_with = read_pos_vec3)]
    #[bw(write_with = write_pos_vec3)]
    pub pos: Vec3,
}

impl From<Vec3> for PositionKey {
    fn from(pos: Vec3) -> Self {
        Self { time: 0, pos }
    }
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JointData {
    #[br(parse_with = read_null_terminated_string)]
    #[bw(write_with = write_null_terminated_string)]
    pub name: String,
    pub priority: i32,

    #[br(temp)]
    #[bw(calc = rotation_keys.len() as i32)]
    num_rot_keys: i32,
    #[br(count = num_rot_keys)]
    pub rotation_keys: Vec<RotationKey>,

    #[br(temp)]
    #[bw(calc = position_keys.len() as i32)]
    num_pos_keys: i32,
    #[br(count = num_pos_keys)]
    pub position_keys: Vec<PositionKey>,
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constraint {
    pub chain_length: u8,
    pub constraint_type: u8,

    #[br(count = 16)]
    #[br(parse_with = read_fixed_length_string)]
    #[bw(write_with = write_fixed_length_string)]
    pub source_volume: String,

    pub source_offset: [f32; 3],

    #[br(count = 16)]
    #[br(parse_with = read_fixed_length_string)]
    #[bw(write_with = write_fixed_length_string)]
    pub target_volume: String,

    pub target_offset: [f32; 3],
    pub target_dir: [f32; 3],
    pub ease_in_start: f32,
    pub ease_in_stop: f32,
    pub ease_out_start: f32,
    pub ease_out_stop: f32,
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animation {
    pub header: AnimationHeader,

    #[br(temp)]
    #[bw(calc = joints.len() as u32)]
    num_joints: u32,
    #[br(count = num_joints)]
    pub joints: Vec<JointData>,

    #[br(temp)]
    #[bw(calc = constraints.len() as i32)]
    num_constraints: i32,
    #[br(count = num_constraints)]
    pub constraints: Vec<Constraint>,
}

impl Animation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_priority(&mut self, priority: i32) -> &mut Self {
        self.header.base_priority = priority;
        for joint in &mut self.joints {
            joint.priority = priority;
        }
        self
    }

    pub fn set_joint_priority(&mut self, priority: i32) -> &mut Self {
        for joint in &mut self.joints {
            joint.priority = priority;
        }
        self
    }

    pub fn drop_empty_joints(&mut self) -> &mut Self {
        self.joints
            .retain(|joint| !joint.position_keys.is_empty() || !joint.rotation_keys.is_empty());
        self
    }

    pub fn drop_position_keys(&mut self) -> &mut Self {
        for joint in &mut self.joints {
            joint.position_keys.clear();
        }
        self
    }

    pub fn drop_position(&mut self, joints: impl Fn(&JointData) -> bool) -> &mut Self {
        for joint in &mut self.joints {
            if joints(joint) {
                joint.position_keys.clear();
            }
        }
        self
    }

    pub fn drop_rotation_keys(&mut self) -> &mut Self {
        for joint in &mut self.joints {
            joint.rotation_keys.clear();
        }
        self
    }

    pub fn drop_rotation(&mut self, joints: impl Fn(&JointData) -> bool) -> &mut Self {
        for joint in &mut self.joints {
            if joints(joint) {
                joint.rotation_keys.clear();
            }
        }
        self
    }

    pub fn cleanup_keys(&mut self) -> &mut Self {
        for joint in &mut self.joints {
            let mut seen_times = HashSet::new();
            joint.rotation_keys.reverse();
            joint
                .rotation_keys
                .retain(|key| seen_times.insert(key.time));
            joint.rotation_keys.sort_by_key(|key| key.time);
            seen_times.clear();
            joint.position_keys.reverse();
            joint
                .position_keys
                .retain(|key| seen_times.insert(key.time));
            joint.position_keys.sort_by_key(|key| key.time);
        }
        self
    }

    pub fn joint(&self, name: &str) -> Option<&JointData> {
        self.joints.iter().find(|joint| joint.name == name)
    }

    pub fn joint_mut(&mut self, name: &str) -> Option<&mut JointData> {
        self.joints.iter_mut().find(|joint| joint.name == name)
    }

    /// Creates an animation from LLSD data, typically from Firestorm poser files.
    ///
    /// This function parses LLSD-XML data exported by Firestorm's poser system and converts
    /// it into an animation structure. Firestorm stores pose data in LLSD-XML format in
    /// the user's configuration directory.
    ///
    /// # Arguments
    ///
    /// * `llsd` - The parsed LLSD data containing joint poses
    /// * `check_enabled` - If true, only includes joints where the "enabled" field is true
    ///
    /// # File Locations
    ///
    /// Firestorm poser files are typically found at:
    /// * **Linux**: `~/.firestorm_x64/user_settings/poses/`
    /// * **Windows**: `%APPDATA%/Firestorm_x64/user_settings/poses/`
    /// * **macOS**: `~/Library/Application Support/Firestorm_x64/user_settings/poses/`
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::fs::File;
    /// use std::io::BufReader;
    /// use avatar_anim::Animation;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Load LLSD-XML file from Firestorm poses directory
    /// let file = BufReader::new(File::open("my_pose.xml")?);
    /// let llsd = llsd_rs::xml::from_reader(file)?;
    ///
    /// // Convert to animation, including only enabled joints
    /// let animation = Animation::from_llsd(&llsd, true)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_llsd(llsd: &Llsd, check_enabled: bool) -> Result<Self, String> {
        let Some(joints) = llsd.as_map() else {
            return Err("LLDS must be a map".into());
        };
        let mut animation = Self::default();
        for (key, value) in joints {
            let Some(inner) = value.as_map() else {
                continue;
            };
            if check_enabled
                && inner
                    .get("enabled")
                    .is_none_or(|e| e.as_boolean() != Some(&true))
            {
                continue;
            }
            let extract_key = |key: &str| -> Option<(f32, f32, f32)> {
                inner.get(key).and_then(|e| e.as_array()).map(|arr| {
                    (
                        *arr.first().and_then(|e| e.as_real()).unwrap_or(&0.0f64) as f32,
                        *arr.get(1).and_then(|e| e.as_real()).unwrap_or(&0.0f64) as f32,
                        *arr.get(2).and_then(|e| e.as_real()).unwrap_or(&0.0f64) as f32,
                    )
                })
            };
            let rotation = extract_key("rotation").map(|(roll, pitch, yaw)| RotationKey {
                time: u16::MAX,
                rot: Quat::from_euler(EulerRot::XYZ, roll, pitch, yaw).normalize(),
            });
            let position = extract_key("position").map(|(x, y, z)| PositionKey {
                time: u16::MAX,
                pos: Vec3::new(x, y, z),
            });
            animation.joints.push(JointData {
                name: key.clone(),
                rotation_keys: rotation.into_iter().collect(),
                position_keys: position.into_iter().collect(),
                ..Default::default()
            });
        }
        Ok(animation)
    }

    /// Load an animation from a .anim file
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use avatar_anim::Animation;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let animation = Animation::from_file("my_animation.anim")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, binrw::Error> {
        use binrw::BinRead;
        use std::fs::File;
        use std::io::BufReader;

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        Self::read(&mut reader)
    }

    /// Save an animation to a .anim file
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use avatar_anim::Animation;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let animation = Animation::default();
    /// animation.to_file("output.anim")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), binrw::Error> {
        use binrw::BinWrite;
        use std::fs::File;
        use std::io::BufWriter;

        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        self.write(&mut writer)
    }

    /// Load LLSD-XML data from a Firestorm pose file
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use avatar_anim::Animation;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let animation = Animation::from_llsd_file("pose.xml", true)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_llsd_file<P: AsRef<std::path::Path>>(
        path: P,
        check_enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use std::fs::File;
        use std::io::BufReader;

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let llsd = llsd_rs::xml::from_reader(reader)?;
        Ok(Self::from_llsd(&llsd, check_enabled)?)
    }
}
