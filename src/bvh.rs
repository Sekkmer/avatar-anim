//! Dependency-free BVH parsing and Second Life animation conversion.
//!
//! Parsing is intentionally separate from conversion: [`BvhDocument`] accepts
//! ordinary BVH hierarchy and motion data, while [`BvhDocument::to_animation`]
//! applies the coordinate, reference-frame, alias, and timing conventions used
//! by the Linden Lab/Firestorm BVH loader.

use crate::SkeletonDefinition;
use crate::{AnimError, Animation, AnimationHeader, JointData, PositionKey, Result, RotationKey};
use glam::{Quat, Vec3};
use std::collections::HashSet;

const MAX_JOINTS: usize = 1_024;
const MAX_HIERARCHY_DEPTH: usize = 256;
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 100_000;
const MAX_MOTION_VALUES: usize = 10_000_000;
const INCHES_TO_METERS: f32 = 0.025_400_05;
const MAX_POSITION: f32 = 5.0;
const POSITION_MOTION_THRESHOLD: f32 = 0.001;
const ROTATION_MOTION_THRESHOLD: f32 = 0.001;
const POSITION_KEYFRAME_THRESHOLD_METERS: f32 = 0.03 * INCHES_TO_METERS;
const ROTATION_KEYFRAME_THRESHOLD_RADIANS: f32 = 0.01;

/// One of the six channel types defined by BVH.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BvhChannel {
    Xposition,
    Yposition,
    Zposition,
    Xrotation,
    Yrotation,
    Zrotation,
}

impl BvhChannel {
    fn parse(token: &Token) -> Result<Self> {
        match token.text.to_ascii_lowercase().as_str() {
            "xposition" => Ok(Self::Xposition),
            "yposition" => Ok(Self::Yposition),
            "zposition" => Ok(Self::Zposition),
            "xrotation" => Ok(Self::Xrotation),
            "yrotation" => Ok(Self::Yrotation),
            "zrotation" => Ok(Self::Zrotation),
            _ => Err(token.error(format!("unknown BVH channel '{}'", token.text))),
        }
    }

    fn is_position(self) -> bool {
        matches!(self, Self::Xposition | Self::Yposition | Self::Zposition)
    }

    fn is_rotation(self) -> bool {
        matches!(self, Self::Xrotation | Self::Yrotation | Self::Zrotation)
    }
}

/// A hierarchy joint in source-file order.
#[derive(Clone, Debug, PartialEq)]
pub struct BvhJoint {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub offset: Vec3,
    pub channels: Vec<BvhChannel>,
    pub channel_offset: usize,
    pub end_site: Option<Vec3>,
}

/// One flattened BVH motion frame. Values follow hierarchy/channel order.
#[derive(Clone, Debug, PartialEq)]
pub struct BvhFrame {
    pub values: Vec<f32>,
}

/// Parsed BVH hierarchy and motion data.
#[derive(Clone, Debug, PartialEq)]
pub struct BvhDocument {
    pub joints: Vec<BvhJoint>,
    pub frames: Vec<BvhFrame>,
    pub frame_time: f32,
    pub total_channels: usize,
}

/// How conversion handles joints that are absent from the LL skeleton aliases.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnknownJointPolicy {
    /// Preserve the source name and values, matching the viewer loader.
    #[default]
    Preserve,
    /// Silently omit unknown joints.
    Ignore,
    /// Reject conversion with a descriptive error.
    Error,
}

/// Settings supplied by the viewer upload UI rather than the BVH itself.
#[derive(Clone, Debug, PartialEq)]
pub struct BvhImportOptions {
    pub priority: i32,
    pub looped: bool,
    pub ease_in_duration: f32,
    pub ease_out_duration: f32,
    pub hand_pose: u32,
    pub emote_name: String,
    /// Convert BVH position units to metres. The viewer uses inches.
    pub position_scale: f32,
    /// Remove linearly redundant position and rotation keys.
    pub optimize: bool,
    pub unknown_joints: UnknownJointPolicy,
}

impl Default for BvhImportOptions {
    fn default() -> Self {
        Self {
            priority: 2,
            looped: false,
            ease_in_duration: 0.3,
            ease_out_duration: 0.3,
            hand_pose: 1,
            emote_name: String::new(),
            position_scale: INCHES_TO_METERS,
            optimize: true,
            unknown_joints: UnknownJointPolicy::Preserve,
        }
    }
}

impl BvhDocument {
    /// Parse a BVH document from UTF-8 bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        Parser::new(data)?.parse()
    }

    /// Parse a BVH document from a filesystem path.
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let data = std::fs::read(path).map_err(AnimError::Io)?;
        Self::parse(&data)
    }

    /// Values belonging to `joint_index` in `frame_index`.
    pub fn joint_values(&self, frame_index: usize, joint_index: usize) -> Option<&[f32]> {
        let frame = self.frames.get(frame_index)?;
        let joint = self.joints.get(joint_index)?;
        frame
            .values
            .get(joint.channel_offset..joint.channel_offset + joint.channels.len())
    }

    /// Convert into Second Life's internal animation representation.
    ///
    /// Frame zero is treated as the reference frame and is not emitted, as in
    /// `LLBVHLoader`. At least two frames are therefore required.
    pub fn to_animation(
        &self,
        skeleton: &SkeletonDefinition,
        options: &BvhImportOptions,
    ) -> Result<Animation> {
        if self.frames.len() < 2 {
            return Err(AnimError::BvhConversion(
                "Second Life BVH conversion requires a reference frame and at least one motion frame"
                    .to_owned(),
            ));
        }
        validate_options(options)?;

        let duration = ((self.frames.len().saturating_sub(2)).max(1) as f32 * self.frame_time)
            .max(f32::EPSILON);
        if duration > 60.0 {
            return Err(AnimError::BvhConversion(format!(
                "animation duration {duration:.3}s exceeds Second Life's 60s limit"
            )));
        }
        let (ease_in_duration, ease_out_duration) =
            if !options.looped && options.ease_in_duration + options.ease_out_duration > duration {
                let factor = duration / (options.ease_in_duration + options.ease_out_duration);
                (
                    options.ease_in_duration * factor,
                    options.ease_out_duration * factor,
                )
            } else {
                (options.ease_in_duration, options.ease_out_duration)
            };

        let root_name = skeleton.canonical_name(&self.joints[0].name);
        if root_name != Some("mPelvis") {
            return Err(AnimError::BvhConversion(format!(
                "BVH root '{}' must be mPelvis, hip, or another mPelvis alias",
                self.joints[0].name
            )));
        }

        let mut animation = Animation {
            header: AnimationHeader {
                base_priority: options.priority,
                duration,
                emote_name: options.emote_name.clone(),
                loop_in_point: 0.0,
                loop_out_point: duration,
                looped: i32::from(options.looped),
                ease_in_duration,
                ease_out_duration,
                hand_pose: options.hand_pose,
                ..AnimationHeader::default()
            },
            ..Animation::default()
        };

        for (joint_index, joint) in self.joints.iter().enumerate() {
            let canonical = skeleton.canonical_name(&joint.name);
            let output_name = match (canonical, options.unknown_joints) {
                (Some(name), _) => name.to_owned(),
                (None, UnknownJointPolicy::Preserve) => joint.name.clone(),
                (None, UnknownJointPolicy::Ignore) => continue,
                (None, UnknownJointPolicy::Error) => {
                    return Err(AnimError::BvhConversion(format!(
                        "BVH joint '{}' is not present in the LL skeleton or its aliases",
                        joint.name
                    )));
                }
            };
            validate_sl_channels(joint)?;

            let known_joint = canonical.is_some();
            let is_pelvis = canonical == Some("mPelvis");
            let reference = self.joint_values(0, joint_index).ok_or_else(|| {
                AnimError::BvhConversion(format!("missing reference values for '{}'", joint.name))
            })?;
            let reference_rotation = source_rotation(joint, reference)?;
            let reference_position = source_position(joint, reference)?;

            let mut rotations = Vec::with_capacity(self.frames.len() - 1);
            let mut positions = Vec::with_capacity(self.frames.len() - 1);
            let mut rotation_changed = false;
            let mut position_changed = false;

            for frame_index in 1..self.frames.len() {
                let values = self.joint_values(frame_index, joint_index).ok_or_else(|| {
                    AnimError::BvhConversion(format!(
                        "missing frame {frame_index} values for '{}'",
                        joint.name
                    ))
                })?;
                let source_rotation = source_rotation(joint, values)?;
                let source_position = source_position(joint, values)?;

                rotation_changed |= quaternion_distance(source_rotation, reference_rotation)
                    > ROTATION_MOTION_THRESHOLD;
                if let (Some(position), Some(reference)) = (source_position, reference_position) {
                    position_changed |= (position - reference).length() > POSITION_MOTION_THRESHOLD;
                }

                let rotation =
                    convert_rotation(source_rotation, reference_rotation, known_joint, is_pelvis);
                let time = frame_time(frame_index, self.frame_time, duration);
                rotations.push(RotationKey {
                    time,
                    rot: canonicalize_quaternion(rotation),
                });

                if let Some(position) = source_position {
                    let converted = convert_position(
                        position,
                        reference_position.unwrap_or(Vec3::ZERO),
                        reference_rotation,
                        known_joint,
                        is_pelvis,
                        options.position_scale,
                    );
                    positions.push(PositionKey {
                        time,
                        pos: converted.clamp(Vec3::splat(-MAX_POSITION), Vec3::splat(MAX_POSITION)),
                    });
                }
            }

            if !(rotation_changed || position_changed) {
                continue;
            }
            if options.optimize {
                rotations = reduce_rotations(rotations, ROTATION_KEYFRAME_THRESHOLD_RADIANS);
                positions = reduce_positions(positions, POSITION_KEYFRAME_THRESHOLD_METERS);
            }
            animation.joints.push(JointData {
                name: output_name,
                priority: options.priority,
                rotation_keys: rotations,
                position_keys: positions,
            });
        }

        animation.cleanup_keys();
        Ok(animation)
    }
}

impl Animation {
    /// Parse and convert BVH bytes using the supplied LL skeleton and options.
    pub fn from_bvh(
        data: &[u8],
        skeleton: &SkeletonDefinition,
        options: &BvhImportOptions,
    ) -> Result<Self> {
        BvhDocument::parse(data)?.to_animation(skeleton, options)
    }

    /// Parse and convert a BVH file from a filesystem path.
    pub fn from_bvh_file<P: AsRef<std::path::Path>>(
        path: P,
        skeleton: &SkeletonDefinition,
        options: &BvhImportOptions,
    ) -> Result<Self> {
        let data = std::fs::read(path).map_err(AnimError::Io)?;
        Self::from_bvh(&data, skeleton, options)
    }
}

fn validate_options(options: &BvhImportOptions) -> Result<()> {
    for (name, value) in [
        ("ease-in duration", options.ease_in_duration),
        ("ease-out duration", options.ease_out_duration),
        ("position scale", options.position_scale),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(AnimError::BvhConversion(format!(
                "{name} must be a finite non-negative number"
            )));
        }
    }
    Ok(())
}

fn validate_sl_channels(joint: &BvhJoint) -> Result<()> {
    let rotations = joint
        .channels
        .iter()
        .filter(|channel| channel.is_rotation())
        .count();
    let positions = joint
        .channels
        .iter()
        .filter(|channel| channel.is_position())
        .count();
    if rotations != 3 || !matches!(positions, 0 | 3) {
        return Err(AnimError::BvhConversion(format!(
            "joint '{}' must have all three rotation channels and either zero or three position channels",
            joint.name
        )));
    }
    Ok(())
}

fn source_rotation(joint: &BvhJoint, values: &[f32]) -> Result<Quat> {
    let mut result = Quat::IDENTITY;
    let mut count = 0;
    for (channel, value) in joint.channels.iter().zip(values) {
        let radians = value.to_radians();
        let rotation = match channel {
            BvhChannel::Xrotation => Some(Quat::from_rotation_x(radians)),
            BvhChannel::Yrotation => Some(Quat::from_rotation_y(radians)),
            BvhChannel::Zrotation => Some(Quat::from_rotation_z(radians)),
            _ => None,
        };
        if let Some(rotation) = rotation {
            // LLBVHLoader reverses the channel string before passing it to
            // mayaQ(), whose multiplication operator is itself reversed.
            // In glam's conventional multiplication this is source order.
            result *= rotation;
            count += 1;
        }
    }
    if count != 3 {
        return Err(AnimError::BvhConversion(format!(
            "joint '{}' does not provide three rotation values",
            joint.name
        )));
    }
    Ok(result.normalize())
}

fn source_position(joint: &BvhJoint, values: &[f32]) -> Result<Option<Vec3>> {
    let mut position = Vec3::ZERO;
    let mut seen = [false; 3];
    for (channel, value) in joint.channels.iter().zip(values) {
        match channel {
            BvhChannel::Xposition => {
                position.x = *value;
                seen[0] = true;
            }
            BvhChannel::Yposition => {
                position.y = *value;
                seen[1] = true;
            }
            BvhChannel::Zposition => {
                position.z = *value;
                seen[2] = true;
            }
            _ => {}
        }
    }
    match seen {
        [false, false, false] => Ok(None),
        [true, true, true] => Ok(Some(position)),
        _ => Err(AnimError::BvhConversion(format!(
            "joint '{}' has incomplete position channels",
            joint.name
        ))),
    }
}

fn sl_basis() -> Quat {
    Quat::from_axis_angle(Vec3::ONE.normalize(), std::f32::consts::TAU / 3.0)
}

fn convert_rotation(source: Quat, reference: Quat, known: bool, pelvis: bool) -> Quat {
    let relative = if pelvis {
        reference.conjugate() * source
    } else {
        source
    };
    if known {
        let basis = sl_basis();
        basis * relative * basis.conjugate()
    } else {
        relative
    }
}

fn convert_position(
    source: Vec3,
    reference: Vec3,
    reference_rotation: Quat,
    known: bool,
    pelvis: bool,
    scale: f32,
) -> Vec3 {
    let position = if pelvis {
        reference_rotation.conjugate() * (source - reference)
    } else {
        source
    };
    let position = if known {
        sl_basis() * position
    } else {
        position
    };
    position * scale
}

fn frame_time(frame_index: usize, frame_duration: f32, duration: f32) -> u16 {
    let seconds = frame_index.saturating_sub(1) as f32 * frame_duration;
    ((seconds / duration).clamp(0.0, 1.0) * u16::MAX as f32).round() as u16
}

fn canonicalize_quaternion(rotation: Quat) -> Quat {
    let rotation = rotation.normalize();
    if rotation.w < 0.0 {
        -rotation
    } else {
        rotation
    }
}

fn quaternion_distance(left: Quat, right: Quat) -> f32 {
    2.0 * left.dot(right).abs().clamp(-1.0, 1.0).acos()
}

fn reduce_rotations(keys: Vec<RotationKey>, tolerance: f32) -> Vec<RotationKey> {
    reduce_keys(
        keys,
        |left, middle, right| {
            let span = right.time.saturating_sub(left.time);
            let amount = if span == 0 {
                0.0
            } else {
                middle.time.saturating_sub(left.time) as f32 / span as f32
            };
            quaternion_distance(left.rot.slerp(right.rot, amount), middle.rot)
        },
        tolerance,
    )
}

fn reduce_positions(keys: Vec<PositionKey>, tolerance: f32) -> Vec<PositionKey> {
    reduce_keys(
        keys,
        |left, middle, right| {
            let span = right.time.saturating_sub(left.time);
            let amount = if span == 0 {
                0.0
            } else {
                middle.time.saturating_sub(left.time) as f32 / span as f32
            };
            left.pos.lerp(right.pos, amount).distance(middle.pos)
        },
        tolerance,
    )
}

fn reduce_keys<T: Clone>(
    keys: Vec<T>,
    error: impl Fn(&T, &T, &T) -> f32,
    tolerance: f32,
) -> Vec<T> {
    if keys.len() <= 2 {
        return keys;
    }
    let mut kept = Vec::with_capacity(keys.len());
    kept.push(keys[0].clone());
    for window in keys.windows(3) {
        if error(&window[0], &window[1], &window[2]) > tolerance {
            kept.push(window[1].clone());
        }
    }
    kept.push(keys[keys.len() - 1].clone());
    kept
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    line: usize,
    column: usize,
}

impl Token {
    fn error(&self, message: impl Into<String>) -> AnimError {
        AnimError::BvhParse {
            line: self.line,
            column: self.column,
            message: message.into(),
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    joints: Vec<BvhJoint>,
    names: HashSet<String>,
    total_channels: usize,
}

impl Parser {
    fn new(data: &[u8]) -> Result<Self> {
        if data.len() > MAX_INPUT_BYTES {
            return Err(AnimError::BvhParse {
                line: 1,
                column: 1,
                message: format!(
                    "BVH input is {} bytes; limit is {MAX_INPUT_BYTES}",
                    data.len()
                ),
            });
        }
        let text = std::str::from_utf8(data).map_err(|error| AnimError::BvhParse {
            line: 1,
            column: error.valid_up_to() + 1,
            message: "BVH input is not valid UTF-8".to_owned(),
        })?;
        Ok(Self {
            tokens: tokenize(text),
            cursor: 0,
            joints: Vec::new(),
            names: HashSet::new(),
            total_channels: 0,
        })
    }

    fn parse(mut self) -> Result<BvhDocument> {
        self.expect_keyword("HIERARCHY")?;
        self.expect_keyword("ROOT")?;
        let root_name = self.next_token()?.text.clone();
        self.parse_joint(root_name, None, 0)?;
        self.expect_keyword("MOTION")?;
        self.expect_keyword("Frames")?;
        self.consume_colon();
        let frame_count = self.parse_usize("frame count")?;
        if frame_count == 0 {
            return Err(self.error_here("BVH must contain at least one frame"));
        }
        if frame_count > MAX_FRAMES {
            return Err(self.error_here(format!(
                "BVH declares {frame_count} frames; limit is {MAX_FRAMES}"
            )));
        }
        self.expect_keyword("Frame")?;
        self.expect_keyword("Time")?;
        self.consume_colon();
        let frame_time = self.parse_f32("frame time")?;
        if frame_time <= 0.0 {
            return Err(self.error_here("frame time must be greater than zero"));
        }
        let value_count = frame_count
            .checked_mul(self.total_channels)
            .ok_or_else(|| self.error_here("BVH motion data size overflow"))?;
        if value_count > MAX_MOTION_VALUES {
            return Err(self.error_here(format!(
                "BVH contains {value_count} motion values; limit is {MAX_MOTION_VALUES}"
            )));
        }
        let mut frames = Vec::with_capacity(frame_count);
        for frame in 0..frame_count {
            let mut values = Vec::with_capacity(self.total_channels);
            for channel in 0..self.total_channels {
                values.push(self.parse_f32(&format!(
                    "motion value {} for frame {}",
                    channel + 1,
                    frame + 1
                ))?);
            }
            frames.push(BvhFrame { values });
        }
        if let Some(token) = self.tokens.get(self.cursor) {
            return Err(token.error(format!(
                "unexpected data after the declared {frame_count} motion frames"
            )));
        }
        Ok(BvhDocument {
            joints: self.joints,
            frames,
            frame_time,
            total_channels: self.total_channels,
        })
    }

    fn parse_joint(&mut self, name: String, parent: Option<usize>, depth: usize) -> Result<usize> {
        if depth > MAX_HIERARCHY_DEPTH {
            return Err(self.error_here(format!(
                "BVH hierarchy exceeds {MAX_HIERARCHY_DEPTH} levels"
            )));
        }
        if self.joints.len() >= MAX_JOINTS {
            return Err(self.error_here(format!("BVH exceeds {MAX_JOINTS} joints")));
        }
        if !self.names.insert(name.clone()) {
            return Err(self.error_here(format!("duplicate BVH joint name '{name}'")));
        }
        self.expect("{")?;
        self.expect_keyword("OFFSET")?;
        let offset = self.parse_offset("joint offset", "CHANNELS")?;
        self.expect_keyword("CHANNELS")?;
        let channel_count = self.parse_usize("channel count")?;
        if channel_count > 6 {
            return Err(self.error_here(format!(
                "joint '{name}' declares {channel_count} channels; BVH supports at most 6"
            )));
        }
        let channel_offset = self.total_channels;
        let mut channels = Vec::with_capacity(channel_count);
        let mut unique = HashSet::new();
        for _ in 0..channel_count {
            let token = self.next_token()?.clone();
            let channel = BvhChannel::parse(&token)?;
            if !unique.insert(channel) {
                return Err(token.error(format!("duplicate channel '{}'", token.text)));
            }
            channels.push(channel);
        }
        self.total_channels = self
            .total_channels
            .checked_add(channels.len())
            .ok_or_else(|| self.error_here("BVH channel count overflow"))?;

        let index = self.joints.len();
        self.joints.push(BvhJoint {
            name,
            parent,
            children: Vec::new(),
            offset,
            channels,
            channel_offset,
            end_site: None,
        });
        loop {
            if self.peek_keyword("JOINT") {
                self.cursor += 1;
                let child_name = self.next_token()?.text.clone();
                let child = self.parse_joint(child_name, Some(index), depth + 1)?;
                self.joints[index].children.push(child);
            } else if self.peek_keyword("End") {
                self.cursor += 1;
                self.expect_keyword("Site")?;
                if self.joints[index].end_site.is_some() {
                    return Err(self.error_here("joint has more than one End Site"));
                }
                self.expect("{")?;
                self.expect_keyword("OFFSET")?;
                self.joints[index].end_site = Some(self.parse_offset("End Site offset", "}")?);
                self.expect("}")?;
            } else if self.peek("}") {
                self.cursor += 1;
                break;
            } else {
                let token = self.next_token()?.clone();
                return Err(token.error(format!(
                    "expected JOINT, End Site, or '}}', found '{}'",
                    token.text
                )));
            }
        }
        Ok(index)
    }

    fn parse_vec3(&mut self, description: &str) -> Result<Vec3> {
        Ok(Vec3::new(
            self.parse_f32(description)?,
            self.parse_f32(description)?,
            self.parse_f32(description)?,
        ))
    }

    fn parse_offset(&mut self, description: &str, empty_terminator: &str) -> Result<Vec3> {
        if self
            .tokens
            .get(self.cursor)
            .is_some_and(|token| token.text.eq_ignore_ascii_case(empty_terminator))
        {
            // Firestorm can emit empty OFFSET lines for legacy placeholder
            // joints. LLBVHLoader only checks for the keyword and accepts them.
            return Ok(Vec3::ZERO);
        }
        self.parse_vec3(description)
    }

    fn parse_f32(&mut self, description: &str) -> Result<f32> {
        let token = self.next_token()?.clone();
        let value = token.text.parse::<f32>().map_err(|_| {
            token.error(format!(
                "expected finite number for {description}, found '{}'",
                token.text
            ))
        })?;
        if !value.is_finite() {
            return Err(token.error(format!("{description} must be finite")));
        }
        Ok(value)
    }

    fn parse_usize(&mut self, description: &str) -> Result<usize> {
        let token = self.next_token()?.clone();
        token.text.parse::<usize>().map_err(|_| {
            token.error(format!(
                "expected non-negative integer for {description}, found '{}'",
                token.text
            ))
        })
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<()> {
        let token = self.next_token()?.clone();
        if token.text.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            Err(token.error(format!("expected {expected}, found '{}'", token.text)))
        }
    }

    fn expect(&mut self, expected: &str) -> Result<()> {
        let token = self.next_token()?.clone();
        if token.text == expected {
            Ok(())
        } else {
            Err(token.error(format!("expected '{expected}', found '{}'", token.text)))
        }
    }

    fn consume_colon(&mut self) {
        if self.peek(":") {
            self.cursor += 1;
        }
    }

    fn peek_keyword(&self, expected: &str) -> bool {
        self.tokens
            .get(self.cursor)
            .is_some_and(|token| token.text.eq_ignore_ascii_case(expected))
    }

    fn peek(&self, expected: &str) -> bool {
        self.tokens
            .get(self.cursor)
            .is_some_and(|token| token.text == expected)
    }

    fn next_token(&mut self) -> Result<&Token> {
        let token = self.tokens.get(self.cursor).ok_or_else(|| {
            let (line, column) = self.tokens.last().map_or((1, 1), |token| {
                (token.line, token.column + token.text.len())
            });
            AnimError::BvhParse {
                line,
                column,
                message: "unexpected end of BVH input".to_owned(),
            }
        })?;
        self.cursor += 1;
        Ok(token)
    }

    fn error_here(&self, message: impl Into<String>) -> AnimError {
        let message = message.into();
        if let Some(token) = self.tokens.get(self.cursor) {
            token.error(message)
        } else {
            AnimError::BvhParse {
                line: self.tokens.last().map_or(1, |token| token.line),
                column: self
                    .tokens
                    .last()
                    .map_or(1, |token| token.column + token.text.len()),
                message,
            }
        }
    }
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.trim_start_matches('\u{feff}').chars().peekable();
    let mut line = 1;
    let mut column = 1;
    while let Some(character) = chars.next() {
        if character == '\n' {
            line += 1;
            column = 1;
            continue;
        }
        if character.is_whitespace() {
            column += 1;
            continue;
        }
        if character == '#' {
            for next in chars.by_ref() {
                if next == '\n' {
                    line += 1;
                    column = 1;
                    break;
                }
            }
            continue;
        }
        if character == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    line += 1;
                    column = 1;
                    break;
                }
            }
            continue;
        }
        let start_column = column;
        if matches!(character, '{' | '}' | ':') {
            tokens.push(Token {
                text: character.to_string(),
                line,
                column,
            });
            column += 1;
            continue;
        }
        let mut text = String::from(character);
        column += 1;
        while let Some(next) = chars.peek().copied() {
            if next.is_whitespace() || matches!(next, '{' | '}' | ':') {
                break;
            }
            text.push(next);
            chars.next();
            column += 1;
        }
        tokens.push(Token {
            text,
            line,
            column: start_column,
        });
    }
    tokens
}
