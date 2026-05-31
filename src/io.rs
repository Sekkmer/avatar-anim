use binrw::{
    BinRead, BinResult, Endian,
    io::{Read, Seek, Write},
};
use glam::{EulerRot, Quat, Vec3};
use std::string::FromUtf8Error;

use crate::{PositionKey, RotationKey};

pub(crate) const KEYFRAME_MOTION_VERSION: u16 = 1;
pub(crate) const KEYFRAME_MOTION_SUBVERSION: u16 = 0;
const KEYFRAME_MOTION_OLD_VERSION: u16 = 0;
const KEYFRAME_MOTION_OLD_SUBVERSION: u16 = 1;
pub(crate) const MAX_PELVIS_OFFSET: f32 = 5.0;

const OOU16MAX: f32 = 1.0f32 / u16::MAX as f32;

fn clamp(value: f32, lower: f32, upper: f32) -> f32 {
    value.min(upper).max(lower)
}

pub(crate) fn f32_to_u16(value: f32, lower: f32, upper: f32) -> u16 {
    let mut val = clamp(value, lower, upper);
    val -= lower;
    val /= upper - lower;
    (val * u16::MAX as f32).floor() as u16
}

pub(crate) fn u16_to_f32(value: u16, lower: f32, upper: f32) -> f32 {
    let mut val = value as f32 * OOU16MAX;
    let delta = upper - lower;
    val *= delta;
    val += lower;

    let max_error = delta * OOU16MAX;
    if val.abs() < max_error {
        val = 0.0;
    }

    val
}

pub fn read_null_terminated_string<R: Read + Seek>(
    r: &mut R,
    _: Endian,
    _: (),
) -> BinResult<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        r.read_exact(&mut byte)?;
        if byte[0] == 0 {
            break;
        }
        buf.push(byte[0]);
    }
    String::from_utf8(buf).map_err(|e: FromUtf8Error| binrw::Error::AssertFail {
        pos: 0,
        message: format!("Invalid UTF-8 in null-terminated string: {e}"),
    })
}

pub fn write_null_terminated_string<W: Write + Seek>(
    data: &String,
    w: &mut W,
    _: Endian,
    _: (),
) -> BinResult<()> {
    w.write_all(data.as_bytes())?;
    w.write_all(&[0])?;
    Ok(())
}

pub fn read_fixed_length_string<R: Read + Seek>(
    r: &mut R,
    _: Endian,
    args: (usize,),
) -> BinResult<String> {
    let length = args.0;
    let mut buf = vec![0u8; length];
    r.read_exact(&mut buf)?;
    if let Some(pos) = buf.iter().position(|&b| b == 0) {
        buf.truncate(pos);
    }
    Ok(String::from_utf8(buf).unwrap_or_default())
}

pub fn write_fixed_length_string<W: Write + Seek>(
    data: &String,
    w: &mut W,
    _: Endian,
    args: (usize,),
) -> BinResult<()> {
    let length = args.0;
    let mut buf = data.as_bytes().to_vec();
    buf.resize(length, 0);
    w.write_all(&buf)?;
    Ok(())
}

pub fn read_rot_quat<R: Read + Seek>(reader: &mut R, e: Endian, _: ()) -> BinResult<Quat> {
    use binrw::BinRead;
    let x: f32 = u16_to_f32(u16::read_options(reader, e, ())?, -1.0, 1.0);
    let y: f32 = u16_to_f32(u16::read_options(reader, e, ())?, -1.0, 1.0);
    let z: f32 = u16_to_f32(u16::read_options(reader, e, ())?, -1.0, 1.0);
    let sum = x * x + y * y + z * z;
    let w = if sum <= 1.0 { (1.0 - sum).sqrt() } else { 0.0 };
    let mut q = Quat::from_xyzw(x, y, z, w);
    if q.length_squared() > 0.0 {
        q = q.normalize();
    }
    if q.w < 0.0 {
        q = Quat::from_xyzw(-q.x, -q.y, -q.z, -q.w);
    }
    Ok(q)
}

pub fn write_rot_quat<W: Write + Seek>(
    value: &Quat,
    writer: &mut W,
    e: Endian,
    _: (),
) -> BinResult<()> {
    use binrw::BinWrite;
    let mut q = if value.length_squared() > 0.0 {
        value.normalize()
    } else {
        *value
    };
    // Enforce canonical hemisphere (positive w) for stable roundtrips
    if q.w < 0.0 {
        q = Quat::from_xyzw(-q.x, -q.y, -q.z, -q.w);
    }
    f32_to_u16(q.x, -1.0, 1.0).write_options(writer, e, ())?;
    f32_to_u16(q.y, -1.0, 1.0).write_options(writer, e, ())?;
    f32_to_u16(q.z, -1.0, 1.0).write_options(writer, e, ())
}

pub fn read_pos_vec3<R: Read + Seek>(reader: &mut R, e: Endian, _: ()) -> BinResult<Vec3> {
    use binrw::BinRead;
    let x = u16_to_f32(
        u16::read_options(reader, e, ())?,
        -MAX_PELVIS_OFFSET,
        MAX_PELVIS_OFFSET,
    );
    let y = u16_to_f32(
        u16::read_options(reader, e, ())?,
        -MAX_PELVIS_OFFSET,
        MAX_PELVIS_OFFSET,
    );
    let z = u16_to_f32(
        u16::read_options(reader, e, ())?,
        -MAX_PELVIS_OFFSET,
        MAX_PELVIS_OFFSET,
    );
    Ok(Vec3::new(x, y, z))
}

pub fn write_pos_vec3<W: Write + Seek>(
    value: &Vec3,
    writer: &mut W,
    e: Endian,
    _: (),
) -> BinResult<()> {
    use binrw::BinWrite;
    f32_to_u16(value.x, -MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET).write_options(writer, e, ())?;
    f32_to_u16(value.y, -MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET).write_options(writer, e, ())?;
    f32_to_u16(value.z, -MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET).write_options(writer, e, ())
}

// Quantization helper docs:
// Rotation components are stored as 3 * u16 for x,y,z with range [-1,1]; w is reconstructed.
// Max component absolute quantization error ~= 1 / 65535 * 2 = 3.05e-5 before normalization.
// Position components stored as u16 over [-5,5]; max absolute error ~= 10 / 65535 ≈ 1.53e-4.

pub fn quantize_rotation(q: Quat) -> (u16, u16, u16) {
    let mut qn = if q.length_squared() > 0.0 {
        q.normalize()
    } else {
        q
    };
    if qn.w < 0.0 {
        qn = Quat::from_xyzw(-qn.x, -qn.y, -qn.z, -qn.w);
    }
    (
        f32_to_u16(qn.x, -1.0, 1.0),
        f32_to_u16(qn.y, -1.0, 1.0),
        f32_to_u16(qn.z, -1.0, 1.0),
    )
}

pub fn quantize_position(v: Vec3) -> (u16, u16, u16) {
    (
        f32_to_u16(v.x, -MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET),
        f32_to_u16(v.y, -MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET),
        f32_to_u16(v.z, -MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimVersion {
    Old,
    Current,
}

pub(crate) fn classify_anim_version(version: u16, sub_version: u16) -> BinResult<AnimVersion> {
    if version == KEYFRAME_MOTION_OLD_VERSION && sub_version == KEYFRAME_MOTION_OLD_SUBVERSION {
        Ok(AnimVersion::Old)
    } else if version == KEYFRAME_MOTION_VERSION && sub_version == KEYFRAME_MOTION_SUBVERSION {
        Ok(AnimVersion::Current)
    } else {
        Err(binrw::Error::AssertFail {
            pos: 0,
            message: format!("Unsupported animation version {version}.{sub_version}"),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AnimReadContext {
    pub version: AnimVersion,
    pub duration: f32,
}

fn canonicalize_quat(mut quat: Quat) -> Quat {
    if quat.length_squared() > 0.0 {
        quat = quat.normalize();
    }
    if quat.w < 0.0 {
        quat = Quat::from_xyzw(-quat.x, -quat.y, -quat.z, -quat.w);
    }
    quat
}

fn quat_from_degrees_zyx(x_deg: f32, y_deg: f32, z_deg: f32) -> Quat {
    canonicalize_quat(Quat::from_euler(
        EulerRot::ZYX,
        z_deg.to_radians(),
        y_deg.to_radians(),
        x_deg.to_radians(),
    ))
}

fn quantize_time(time: f32, duration: f32) -> u16 {
    if duration <= 0.0 {
        return 0;
    }
    let clamped = time.clamp(0.0, duration);
    f32_to_u16(clamped, 0.0, duration)
}

pub(crate) fn read_rotation_keys<R: Read + Seek>(
    reader: &mut R,
    endian: Endian,
    count: i32,
    ctx: AnimReadContext,
) -> BinResult<Vec<RotationKey>> {
    if count < 0 {
        return Err(binrw::Error::AssertFail {
            pos: 0,
            message: "num_rot_keys must be non-negative".into(),
        });
    }
    let mut keys = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let time = if ctx.version == AnimVersion::Old {
            let time_seconds: f32 = f32::read_options(reader, endian, ())?;
            quantize_time(time_seconds, ctx.duration)
        } else {
            u16::read_options(reader, endian, ())?
        };

        let rot = if ctx.version == AnimVersion::Old {
            let angles: [f32; 3] = <[f32; 3]>::read_options(reader, endian, ())?;
            quat_from_degrees_zyx(angles[0], angles[1], angles[2])
        } else {
            read_rot_quat(reader, endian, ())?
        };

        keys.push(RotationKey { time, rot });
    }
    Ok(keys)
}

pub(crate) fn read_position_keys<R: Read + Seek>(
    reader: &mut R,
    endian: Endian,
    count: i32,
    ctx: AnimReadContext,
) -> BinResult<Vec<PositionKey>> {
    if count < 0 {
        return Err(binrw::Error::AssertFail {
            pos: 0,
            message: "num_pos_keys must be non-negative".into(),
        });
    }
    let mut keys = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let time = if ctx.version == AnimVersion::Old {
            let time_seconds: f32 = f32::read_options(reader, endian, ())?;
            quantize_time(time_seconds, ctx.duration)
        } else {
            u16::read_options(reader, endian, ())?
        };

        let pos = if ctx.version == AnimVersion::Old {
            let mut components: [f32; 3] = <[f32; 3]>::read_options(reader, endian, ())?;
            for value in &mut components {
                *value = value.clamp(-MAX_PELVIS_OFFSET, MAX_PELVIS_OFFSET);
            }
            Vec3::from_array(components)
        } else {
            read_pos_vec3(reader, endian, ())?
        };

        keys.push(PositionKey { time, pos });
    }
    Ok(keys)
}
