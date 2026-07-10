use avatar_anim::{
    Animation, DuplicateKeyStrategy, JointData, PositionKey, Result, RotationKey, SkeletonBone,
    SkeletonDefinition, bvh::BvhImportOptions,
};
use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write as _};
use std::path::PathBuf;

/// Inspect and manipulate Second Life `.anim`, BVH, and Firestorm poser LLSD XML files.
///
/// Common tasks:
///   animctl info walk.anim
///   animctl list-poses --full
///   animctl clean last input.anim -o cleaned.anim
///   animctl strip rotation input.anim stripped.anim
///   animctl convert -i pose.xml -o pose.anim -p 6 --drop Pelvis,Head
///   animctl convert -i pose.xml -o pose.anim --duration 2.0
///   animctl convert -i pose.xml -o pose.anim --drop-zero-positions --add-skeleton-positions avatar_skeleton.xml
///   animctl convert -i pose.xml --insert Spine:rot<0.1,0.2,0.0>@120 --insert Pelvis:pos<0,0,0.05>
///   animctl convert -i base.anim --drop-rotations --insert Head:rot@42 -o - > head_only.anim
///   animctl skeleton-bones --skeleton avatar_skeleton.xml --prefix mTail
///   animctl position-reset --skeleton avatar_skeleton.xml --prefix mTail -o reset.anim
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
    /// Show a summary of an .anim or BVH animation file
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
    /// Convert / transform poser XML, BVH, or .anim, applying filters & edits
    Convert {
        /// Input file (.xml, .bvh, or .anim)
        #[arg(short = 'i', long = "input", value_hint=ValueHint::FilePath)]
        input: PathBuf,
        /// Optional output file (.anim). Use '-' to write binary .anim to stdout.
        /// If omitted (and not verbose) prints a simple parse success message to stderr.
        #[arg(short = 'o', long = "output", value_hint=ValueHint::FilePath)]
        output: Option<PathBuf>,
        /// Set priority (0..=7) across animation and joints.
        #[arg(short = 'p', long = "priority")]
        priority: Option<i32>,
        /// Set animation duration in seconds and align loop-out to the same value
        #[arg(long = "duration")]
        duration: Option<f32>,
        /// Drop all position keys (after inserts)
        #[arg(long = "drop-positions")]
        drop_positions: bool,
        /// Drop position keys whose delta is exactly zero before adding skeleton positions
        #[arg(long = "drop-zero-positions")]
        drop_zero_positions: bool,
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
        /// Add SL skeleton local bone positions to all position keys.
        /// Use this when converting poser XML positions stored as deltas from avatar_skeleton.xml.
        #[arg(long = "add-skeleton-positions", value_hint=ValueHint::FilePath)]
        add_skeleton_positions: Option<PathBuf>,
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
    /// List bones loaded from SL avatar_skeleton.xml
    SkeletonBones {
        /// SL avatar_skeleton.xml to inspect
        #[arg(short = 's', long = "skeleton", value_hint=ValueHint::FilePath)]
        skeleton: PathBuf,
        /// Include exact bone names (repeatable or comma separated)
        #[arg(long = "joint", value_delimiter = ',')]
        joints: Vec<String>,
        /// Include bone names with this prefix (repeatable or comma separated)
        #[arg(long = "prefix", value_delimiter = ',')]
        prefixes: Vec<String>,
        /// Include bones in this skeleton group (repeatable or comma separated)
        #[arg(long = "group", value_delimiter = ',')]
        groups: Vec<String>,
        /// List every bone with a position
        #[arg(long = "all")]
        all: bool,
    },
    /// Create a position-only reset animation from selected SL skeleton bones
    #[command(alias = "tail-reset")]
    PositionReset {
        /// SL avatar_skeleton.xml to read default local positions from
        #[arg(short = 's', long = "skeleton", value_hint=ValueHint::FilePath)]
        skeleton: PathBuf,
        /// Output .anim path
        #[arg(short = 'o', long = "output", default_value = "reset.anim", value_hint=ValueHint::FilePath)]
        output: PathBuf,
        /// Animation and joint priority
        #[arg(short = 'p', long = "priority", default_value_t = 6)]
        priority: i32,
        /// Include exact bone names (repeatable or comma separated)
        #[arg(long = "joint", value_delimiter = ',')]
        joints: Vec<String>,
        /// Include bone names with this prefix (repeatable or comma separated)
        #[arg(long = "prefix", value_delimiter = ',')]
        prefixes: Vec<String>,
        /// Include bones in this skeleton group (repeatable or comma separated)
        #[arg(long = "group", value_delimiter = ',')]
        groups: Vec<String>,
        /// Include every bone with a position
        #[arg(long = "all")]
        all: bool,
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
            duration,
            drop_positions,
            drop_zero_positions,
            drop_rotations,
            drop_position_named,
            drop_rotation_named,
            drop_joints,
            add_skeleton_positions,
            verbose,
            insert,
        } => {
            cmd_convert(
                input,
                output,
                priority,
                duration,
                drop_positions,
                drop_zero_positions,
                drop_rotations,
                drop_position_named,
                drop_rotation_named,
                drop_joints,
                add_skeleton_positions,
                verbose,
                insert,
            )?;
        }
        Commands::Joints {
            file,
            joint,
            summary,
        } => cmd_joints(file, joint, summary)?,
        Commands::SkeletonBones {
            skeleton,
            joints,
            prefixes,
            groups,
            all,
        } => cmd_skeleton_bones(skeleton, joints, prefixes, groups, all)?,
        Commands::PositionReset {
            skeleton,
            output,
            priority,
            joints,
            prefixes,
            groups,
            all,
        } => cmd_position_reset(skeleton, output, priority, joints, prefixes, groups, all)?,
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
    let anim = load_animation(&path)?;
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

fn is_bvh(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bvh"))
}

fn load_animation(path: &std::path::Path) -> Result<Animation> {
    if is_xml(path) {
        Animation::from_llsd_file(path, true)
    } else if is_bvh(path) {
        let skeleton = SkeletonDefinition::embedded_avatar()?;
        Animation::from_bvh_file(path, &skeleton, &BvhImportOptions::default())
    } else {
        Animation::from_file(path)
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_convert(
    input: PathBuf,
    output: Option<PathBuf>,
    priority: Option<i32>,
    duration: Option<f32>,
    drop_positions: bool,
    drop_zero_positions: bool,
    drop_rotations: bool,
    drop_position_named: Option<String>,
    drop_rotation_named: Option<String>,
    drop_joints: Option<String>,
    add_skeleton_positions: Option<PathBuf>,
    verbose: bool,
    inserts: Vec<String>,
) -> Result<()> {
    let mut anim = load_animation(&input)?;

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
    if drop_zero_positions {
        anim.drop_zero_position_keys(1e-6).drop_empty_joints();
    }
    if drop_rotations {
        anim.drop_rotation_keys();
    }

    if let Some(skeleton_path) = add_skeleton_positions {
        let skeleton = SkeletonDefinition::from_xml_file(skeleton_path)?;
        anim.add_skeleton_positions(&skeleton)?;
    }

    if let Some(p) = priority {
        anim.set_priority(p.clamp(0, 7));
    }
    if let Some(duration) = duration {
        anim.set_duration(duration);
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
        if out.as_os_str() == "-" {
            // Write raw .anim binary to stdout
            let mut buf: Vec<u8> = Vec::new();
            {
                use binrw::{BinWrite, Endian};
                let mut cursor = std::io::Cursor::new(&mut buf);
                anim.write_options(&mut cursor, Endian::Little, ())
                    .map_err(avatar_anim::AnimError::BinRw)?;
            }
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&buf).map_err(avatar_anim::AnimError::Io)?;
        } else {
            anim.to_file(&out)?;
            if !verbose {
                let mut stderr = io::stderr();
                writeln!(stderr, "Wrote animation to {}", out.display()).ok();
            }
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
    let anim = load_animation(&file)?;
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

fn cmd_skeleton_bones(
    skeleton: PathBuf,
    joints: Vec<String>,
    prefixes: Vec<String>,
    groups: Vec<String>,
    all: bool,
) -> Result<()> {
    let skeleton = SkeletonDefinition::from_xml_file(&skeleton)?;
    let bones = select_skeleton_bones(&skeleton, &joints, &prefixes, &groups, all)?;
    for bone in bones {
        let group = bone
            .attributes
            .get("group")
            .map_or("", std::string::String::as_str);
        let parent = bone.parent.as_deref().unwrap_or("");
        println!(
            "{} pos={:.3},{:.3},{:.3} parent={} group={}",
            bone.name, bone.pos.x, bone.pos.y, bone.pos.z, parent, group
        );
    }
    Ok(())
}

fn cmd_position_reset(
    skeleton: PathBuf,
    output: PathBuf,
    priority: i32,
    joints: Vec<String>,
    prefixes: Vec<String>,
    groups: Vec<String>,
    all: bool,
) -> Result<()> {
    let skeleton = SkeletonDefinition::from_xml_file(&skeleton)?;
    let bones = select_skeleton_bones(&skeleton, &joints, &prefixes, &groups, all)?;
    let anim = Animation::position_reset_from_bones(bones.iter().copied(), priority)?;
    anim.to_file(&output)?;

    let mut stderr = io::stderr();
    writeln!(
        stderr,
        "Wrote position-only reset animation to {}",
        output.display(),
    )
    .ok();
    Ok(())
}

fn select_skeleton_bones<'a>(
    skeleton: &'a SkeletonDefinition,
    joints: &[String],
    prefixes: &[String],
    groups: &[String],
    all: bool,
) -> Result<Vec<&'a SkeletonBone>> {
    if !all && joints.is_empty() && prefixes.is_empty() && groups.is_empty() {
        return Err(avatar_anim::AnimError::InvalidStructure(
            "Select bones with --joint, --prefix, --group, or --all".into(),
        ));
    }

    let joint_names: HashSet<&str> = joints.iter().map(|name| name.as_str()).collect();
    for joint in joints {
        if skeleton.bone(joint).is_none() {
            return Err(avatar_anim::AnimError::InvalidStructure(format!(
                "Skeleton is missing requested bone '{joint}'"
            )));
        }
    }

    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for bone in &skeleton.bones {
        let group_match = bone
            .attributes
            .get("group")
            .is_some_and(|group| groups.iter().any(|wanted| wanted == group));
        let prefix_match = prefixes
            .iter()
            .any(|prefix| bone.name.starts_with(prefix.as_str()));
        let joint_match = joint_names.contains(bone.name.as_str());
        if (all || group_match || prefix_match || joint_match) && seen.insert(bone.name.as_str()) {
            selected.push(bone);
        }
    }

    if selected.is_empty() {
        return Err(avatar_anim::AnimError::InvalidStructure(
            "No skeleton bones matched the selection".into(),
        ));
    }

    Ok(selected)
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
