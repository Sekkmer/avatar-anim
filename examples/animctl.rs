use avatar_anim::{Animation, DuplicateKeyStrategy, JointData, PositionKey, Result, RotationKey};
use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};
use std::fs;
use std::io::{self, Write as _};
use std::path::PathBuf;

/// Inspect and manipulate Second Life `.anim` and Firestorm poser LLSD XML files.
///
/// Common tasks:
///   animctl info walk.anim
///   animctl list-poses --full
///   animctl clean last input.anim -o cleaned.anim
///   animctl strip rotation input.anim stripped.anim
///   animctl convert -i pose.xml -o pose.anim -p 6 --drop Pelvis,Head
///   animctl convert -i pose.xml --insert Spine:rot<0.1,0.2,0.0>@120 --insert Pelvis:pos<0,0,0.05>
///   animctl convert -i base.anim --drop-rotations --insert Head:rot@42 -o - > head_only.anim
///
/// Use --verbose on convert for detailed stats and full structure dump to stderr.
#[derive(Parser, Debug)]
#[command(name = "animctl", version, about = "Second Life animation utility", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show a summary of an animation file
    Info {
        #[arg(value_hint=ValueHint::FilePath)]
        file: PathBuf,
    },
    /// Clean duplicate keyframes with strategy
    Clean {
        #[arg(value_enum)]
        strategy: Strategy,
        #[arg(value_hint=ValueHint::FilePath)]
        input: PathBuf,
        #[arg(value_hint=ValueHint::FilePath)]
        output: Option<PathBuf>,
    },
    /// Strip position or rotation keys
    Strip {
        #[arg(value_enum)]
        kind: StripKind,
        #[arg(value_hint=ValueHint::FilePath)]
        input: PathBuf,
        #[arg(value_hint=ValueHint::FilePath)]
        output: PathBuf,
    },
    /// List available Firestorm poser files (LLSD) in default or specified directory
    #[command(alias = "ls")]
    ListPoses {
        /// Override directory (defaults to detected Firestorm poses dir)
        #[arg(short, long)]
        dir: Option<PathBuf>,
        /// Show full paths
        #[arg(long)]
        full: bool,
    },
    /// Convert / transform between poser LLSD XML and .anim, applying filters & edits
    Convert {
        /// Input file (.xml or .anim)
        #[arg(short = 'i', long = "input", value_hint=ValueHint::FilePath)]
        input: PathBuf,
        /// Optional output file (.anim). Use '-' to write binary .anim to stdout.
        /// If omitted (and not verbose) prints a simple parse success message to stderr.
        #[arg(short = 'o', long = "output", value_hint=ValueHint::FilePath)]
        output: Option<PathBuf>,
        /// Set priority (0..=7) across animation and joints.
        #[arg(short = 'p', long = "priority")]
        priority: Option<i32>,
        /// Drop all position keys (after inserts)
        #[arg(long = "drop-positions")]
        drop_positions: bool,
        /// Drop all rotation keys (after inserts)
        #[arg(long = "drop-rotations")]
        drop_rotations: bool,
        /// Drop position keys for named joints (comma separated list)
        #[arg(long = "drop-position")]
        drop_position_named: Option<String>,
        /// Drop rotation keys for named joints (comma separated list)
        #[arg(long = "drop-rotation")]
        drop_rotation_named: Option<String>,
        /// Drop entire joints (comma separated list)
        #[arg(long = "drop")]
        drop_joints: Option<String>,
        /// Verbose: detailed stats + full structure debug to stderr (stdout kept clean for binary output)
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
        /// Insert synthetic key(s) (repeatable)
        #[arg(
            long = "insert",
            value_name = "SPEC",
            long_help = "Insert synthetic key(s). Repeat --insert for multiple.
Syntax:
    joint:pos<x,y,z>[@time]
    joint:rot<roll,pitch,yaw>[@time]

Notes:
    • Angle order is roll(X), pitch(Y), yaw(Z) in radians.
    • <...> block optional; omitted => 0,0,0 (identity rotation / zero position).
    • @time optional; omitted => 65535 (max time, end of animation).

Examples:
    --insert Head:rot<0.1,0.2,0.0>@120
    --insert Pelvis:pos<0,0,0.05>
    --insert Spine:rot@42    (identity rotation at 42)
    --insert Pelvis:pos      (zero position at end)"
        )]
        insert: Vec<String>,
    },
    /// List joints or inspect keys of a specific joint
    Joints {
        /// Animation file (.anim)
        #[arg(value_hint=ValueHint::FilePath)]
        file: PathBuf,
        /// Show compact key list for this joint name instead of all joint names
        #[arg(short = 'j', long = "joint", value_name = "NAME")]
        joint: Option<String>,
        /// Also include a summary count line for each joint when listing all
        #[arg(long = "summary")]
        summary: bool,
    },
    /// Generate shell completion script to stdout
    Complete {
        /// Target shell (bash|zsh|fish|powershell|elvish)
        #[arg(value_enum, short = 's', long = "shell")]
        shell: ShellKind,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ShellKind {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Strategy {
    First,
    Last,
    Average,
}

impl From<Strategy> for DuplicateKeyStrategy {
    fn from(s: Strategy) -> Self {
        match s {
            Strategy::First => DuplicateKeyStrategy::KeepFirst,
            Strategy::Last => DuplicateKeyStrategy::KeepLast,
            Strategy::Average => DuplicateKeyStrategy::Average,
        }
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum StripKind {
    Position,
    Rotation,
    Both,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Info { file } => cmd_info(file)?,
        Commands::Clean {
            strategy,
            input,
            output,
        } => cmd_clean(strategy.into(), input, output)?,
        Commands::Strip {
            kind,
            input,
            output,
        } => cmd_strip(kind, input, output)?,
        Commands::ListPoses { dir, full } => cmd_list_poses(dir, full)?,
        Commands::Convert {
            input,
            output,
            priority,
            drop_positions,
            drop_rotations,
            drop_position_named,
            drop_rotation_named,
            drop_joints,
            verbose,
            insert,
        } => {
            cmd_convert(
                input,
                output,
                priority,
                drop_positions,
                drop_rotations,
                drop_position_named,
                drop_rotation_named,
                drop_joints,
                verbose,
                insert,
            )?;
        }
        Commands::Joints {
            file,
            joint,
            summary,
        } => cmd_joints(file, joint, summary)?,
        Commands::Complete { shell } => cmd_complete(shell)?,
    }
    Ok(())
}

fn firestorm_pose_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME")?;
        let p = PathBuf::from(home).join(".firestorm_x64/user_settings/poses");
        if p.is_dir() {
            return Some(p);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(roaming) = std::env::var_os("APPDATA") {
            let p = PathBuf::from(roaming).join("Firestorm_x64/user_settings/poses");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        let p = PathBuf::from(home)
            .join("Library/Application Support/Firestorm_x64/user_settings/poses");
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn cmd_info(path: PathBuf) -> Result<()> {
    let anim = Animation::from_file(&path)?;
    println!("File: {}", path.display());
    println!(
        "Version: {}.{}",
        anim.header.version, anim.header.sub_version
    );
    println!("Priority: {}", anim.header.base_priority);
    println!("Duration: {:.3}s", anim.header.duration);
    println!("Joints: {}", anim.joints.len());
    let (rot_keys, pos_keys): (usize, usize) = anim.joints.iter().fold((0, 0), |acc, j| {
        (acc.0 + j.rotation_keys.len(), acc.1 + j.position_keys.len())
    });
    println!("Rotation keys: {}  Position keys: {}", rot_keys, pos_keys);
    Ok(())
}

fn cmd_clean(
    strategy: DuplicateKeyStrategy,
    input: PathBuf,
    output: Option<PathBuf>,
) -> Result<()> {
    let mut anim = Animation::from_file(&input)?;
    anim.cleanup_keys_with(strategy);
    let out = output.unwrap_or(input);
    anim.to_file(out)?;
    Ok(())
}

fn cmd_strip(kind: StripKind, input: PathBuf, output: PathBuf) -> Result<()> {
    let mut anim = Animation::from_file(&input)?;
    match kind {
        StripKind::Position => {
            anim.drop_position_keys();
        }
        StripKind::Rotation => {
            anim.drop_rotation_keys();
        }
        StripKind::Both => {
            anim.drop_position_keys().drop_rotation_keys();
        }
    }
    anim.to_file(output)?;
    Ok(())
}

fn cmd_list_poses(dir: Option<PathBuf>, full: bool) -> Result<()> {
    let base = dir.or_else(firestorm_pose_dir).ok_or_else(|| {
        avatar_anim::AnimError::InvalidStructure(
            "Could not determine Firestorm pose directory".into(),
        )
    })?;
    let mut entries: Vec<_> = fs::read_dir(&base)
        .map_err(avatar_anim::AnimError::Io)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "xml"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for ent in entries {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if full {
            println!("{}", ent.path().display());
        } else {
            println!("{}", name);
        }
    }
    Ok(())
}

fn parse_csv_list(input: &Option<String>) -> Vec<String> {
    input
        .as_ref()
        .map(|s| {
            s.split(',')
                .filter(|p| !p.is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn is_xml(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
}

#[allow(clippy::too_many_arguments)]
fn cmd_convert(
    input: PathBuf,
    output: Option<PathBuf>,
    priority: Option<i32>,
    drop_positions: bool,
    drop_rotations: bool,
    drop_position_named: Option<String>,
    drop_rotation_named: Option<String>,
    drop_joints: Option<String>,
    verbose: bool,
    inserts: Vec<String>,
) -> Result<()> {
    let mut anim = if is_xml(&input) {
        // Treat as LLSD XML
        Animation::from_llsd_file(&input, true)?
    } else {
        Animation::from_file(&input)?
    };

    // Process inserts before drops (so dropped joints remove inserted keys if targeted later)
    if !inserts.is_empty() {
        for spec in inserts {
            if let Err(e) = apply_insert(&mut anim, &spec) {
                let mut stderr = io::stderr();
                writeln!(stderr, "Failed to parse --insert '{}': {}", spec, e).ok();
            }
        }
    }

    // Drop entire joints first if requested
    let drop_joint_list = parse_csv_list(&drop_joints);
    if !drop_joint_list.is_empty() {
        anim.joints
            .retain(|j| !drop_joint_list.iter().any(|n| n == &j.name));
    }

    // Named position drops
    let named_pos = parse_csv_list(&drop_position_named);
    if !named_pos.is_empty() {
        let set: std::collections::HashSet<&str> = named_pos.iter().map(|s| s.as_str()).collect();
        anim.drop_position(|j| set.contains(j.name.as_str()));
    }

    // Named rotation drops
    let named_rot = parse_csv_list(&drop_rotation_named);
    if !named_rot.is_empty() {
        let set: std::collections::HashSet<&str> = named_rot.iter().map(|s| s.as_str()).collect();
        anim.drop_rotation(|j| set.contains(j.name.as_str()));
    }

    if drop_positions {
        anim.drop_position_keys();
    }
    if drop_rotations {
        anim.drop_rotation_keys();
    }

    if let Some(p) = priority {
        anim.set_priority(p.clamp(0, 7));
    }

    // Clean duplicates with KeepLast as a sensible default when transforming
    anim.cleanup_keys_with(DuplicateKeyStrategy::KeepLast);

    // If verbose print stats to stderr
    if verbose {
        let total_rot: usize = anim.joints.iter().map(|j| j.rotation_keys.len()).sum();
        let total_pos: usize = anim.joints.iter().map(|j| j.position_keys.len()).sum();
        let mut stderr = io::stderr();
        writeln!(stderr, "Input: {}", input.display()).ok();
        writeln!(stderr, "Joints: {}", anim.joints.len()).ok();
        writeln!(
            stderr,
            "Rotation keys: {} Position keys: {}",
            total_rot, total_pos
        )
        .ok();
        writeln!(stderr, "Priority: {}", anim.header.base_priority).ok();
        writeln!(stderr, "Emote name: {}", anim.header.emote_name).ok();
        writeln!(stderr, "Verbose debug: {:#?}", anim).ok();
    }

    if let Some(out) = output {
        anim.to_file(&out)?;
        // If writing to stdout requested (e.g., '-') treat specially
        if out.as_os_str() == "-" {
            // Write raw .anim binary to stdout
            let mut buf: Vec<u8> = Vec::new();
            {
                use binrw::BinWrite;
                let mut cursor = std::io::Cursor::new(&mut buf);
                anim.write(&mut cursor)
                    .map_err(avatar_anim::AnimError::BinRw)?;
            }
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&buf).map_err(avatar_anim::AnimError::Io)?;
        } else if !verbose {
            let mut stderr = io::stderr();
            writeln!(stderr, "Wrote animation to {}", out.display()).ok();
        }
    } else if !verbose {
        let mut stderr = io::stderr();
        writeln!(stderr, "Parsed animation successfully").ok();
    }

    Ok(())
}

fn apply_insert(anim: &mut Animation, spec: &str) -> std::result::Result<(), String> {
    // Format: joint:pos<x,y,z>[@time]  OR joint:rot<r,p,y>[@time]
    let (left, time_part) = if let Some(idx) = spec.rfind('@') {
        (&spec[..idx], Some(&spec[idx + 1..]))
    } else {
        (spec, None)
    };
    let time: u16 = if let Some(tp) = time_part {
        tp.parse().map_err(|_| "Invalid time")?
    } else {
        u16::MAX
    };
    let mut parts = left.splitn(2, ':');
    let joint = parts.next().ok_or("Missing joint")?;
    let payload = parts.next().ok_or("Missing payload")?;
    let (kind, remainder) = if let Some(rest) = payload.strip_prefix("pos") {
        ("pos", rest)
    } else if let Some(rest) = payload.strip_prefix("rot") {
        ("rot", rest)
    } else {
        return Err("Expected 'pos' or 'rot'".into());
    };
    let mut nums: Vec<f32> = Vec::new();
    if let Some(start) = remainder.strip_prefix('<') {
        let vec_part = start.strip_suffix('>').ok_or("Missing closing '>'")?;
        for part in vec_part.split(',').filter(|s| !s.is_empty()) {
            nums.push(part.trim().parse::<f32>().map_err(|_| "Invalid float")?);
        }
    }
    if kind == "pos" {
        while nums.len() < 3 {
            nums.push(0.0);
        }
        let pos = glam::Vec3::new(nums[0], nums[1], nums[2]);
        ensure_joint(anim, joint)
            .position_keys
            .push(PositionKey { time, pos });
    } else {
        // rot
        while nums.len() < 3 {
            nums.push(0.0);
        }
        let rot =
            glam::Quat::from_euler(glam::EulerRot::XYZ, nums[0], nums[1], nums[2]).normalize();
        ensure_joint(anim, joint)
            .rotation_keys
            .push(RotationKey { time, rot });
    }
    Ok(())
}

fn ensure_joint<'a>(anim: &'a mut Animation, name: &str) -> &'a mut JointData {
    let mut index: Option<usize> = None;
    for (i, j) in anim.joints.iter().enumerate() {
        if j.name == name {
            index = Some(i);
            break;
        }
    }
    if let Some(i) = index {
        return &mut anim.joints[i];
    }
    anim.joints.push(JointData {
        name: name.to_string(),
        ..Default::default()
    });
    let new_index = anim.joints.len() - 1;
    &mut anim.joints[new_index]
}

fn cmd_joints(file: PathBuf, joint: Option<String>, summary: bool) -> Result<()> {
    let anim = Animation::from_file(&file)?;
    if let Some(name) = joint {
        if let Some(j) = anim.joints.iter().find(|j| j.name == name) {
            // Compact format: times+values inline
            // Rotation keys: t: r,p,y (Euler from quat)
            // Position keys: t: x,y,z
            println!("Joint: {}", j.name);
            if !j.rotation_keys.is_empty() {
                print!("rot[");
                for (idx, k) in j.rotation_keys.iter().enumerate() {
                    let (rx, ry, rz) = k.rot.to_euler(glam::EulerRot::XYZ);
                    if idx > 0 {
                        print!(" ");
                    }
                    print!("{}:{:.3},{:.3},{:.3}", k.time, rx, ry, rz);
                }
                println!("]");
            }
            if !j.position_keys.is_empty() {
                print!("pos[");
                for (idx, k) in j.position_keys.iter().enumerate() {
                    if idx > 0 {
                        print!(" ");
                    }
                    print!("{}:{:.3},{:.3},{:.3}", k.time, k.pos.x, k.pos.y, k.pos.z);
                }
                println!("]");
            }
        } else {
            eprintln!("Joint '{}' not found", name);
            return Ok(());
        }
    } else {
        // List all
        for j in &anim.joints {
            if summary {
                println!(
                    "{} (rot:{} pos:{})",
                    j.name,
                    j.rotation_keys.len(),
                    j.position_keys.len()
                );
            } else {
                println!("{}", j.name);
            }
        }
    }
    Ok(())
}

fn cmd_complete(shell: ShellKind) -> Result<()> {
    use clap::CommandFactory;
    use std::io::stdout;
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    match shell {
        ShellKind::Bash => generate(Bash, &mut cmd, &bin_name, &mut stdout()),
        ShellKind::Zsh => generate(Zsh, &mut cmd, &bin_name, &mut stdout()),
        ShellKind::Fish => generate(Fish, &mut cmd, &bin_name, &mut stdout()),
        ShellKind::Powershell => generate(PowerShell, &mut cmd, &bin_name, &mut stdout()),
        ShellKind::Elvish => generate(Elvish, &mut cmd, &bin_name, &mut stdout()),
    }
    Ok(())
}
