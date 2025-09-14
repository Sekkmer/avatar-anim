use binrw::{
    BinResult, Endian, NamedArgs,
    io::{Read, Seek, Write},
};
use glam::{Quat, Vec3};
use std::string::FromUtf8Error;

const OOU16MAX: f32 = 1.0f32 / u16::MAX as f32;

fn clamp(value: f32, lower: f32, upper: f32) -> f32 {
    value.min(upper).max(lower)
}

fn f32_to_u16(value: f32, lower: f32, upper: f32) -> u16 {
    let mut val = clamp(value, lower, upper);
    val -= lower;
    val /= upper - lower;
    (val * u16::MAX as f32).floor() as u16
}

fn u16_to_f32(value: u16, lower: f32, upper: f32) -> f32 {
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

#[derive(NamedArgs, Clone, Default)]
pub struct Args {
    pub count: usize,
}

pub fn read_fixed_length_string<R: Read + Seek>(
    r: &mut R,
    _: Endian,
    args: Args,
) -> BinResult<String> {
    let length = args.count;
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
    args: Args,
) -> BinResult<()> {
    let length = args.count;
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
    let x = u16_to_f32(u16::read_options(reader, e, ())?, -5.0f32, 5.0f32);
    let y = u16_to_f32(u16::read_options(reader, e, ())?, -5.0f32, 5.0f32);
    let z = u16_to_f32(u16::read_options(reader, e, ())?, -5.0f32, 5.0f32);
    Ok(Vec3::new(x, y, z))
}

pub fn write_pos_vec3<W: Write + Seek>(
    value: &Vec3,
    writer: &mut W,
    e: Endian,
    _: (),
) -> BinResult<()> {
    use binrw::BinWrite;
    f32_to_u16(value.x, -5.0f32, 5.0f32).write_options(writer, e, ())?;
    f32_to_u16(value.y, -5.0f32, 5.0f32).write_options(writer, e, ())?;
    f32_to_u16(value.z, -5.0f32, 5.0f32).write_options(writer, e, ())
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
        f32_to_u16(v.x, -5.0, 5.0),
        f32_to_u16(v.y, -5.0, 5.0),
        f32_to_u16(v.z, -5.0, 5.0),
    )
}
