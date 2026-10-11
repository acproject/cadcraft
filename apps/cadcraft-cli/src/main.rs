//! `cadcraft-cli`: headless CADCraft.
//!
//! ```text
//! cadcraft-cli info FILE.dxf                       summary as JSON
//! cadcraft-cli convert IN.dxf OUT.(dxf|svg|png)    convert / export
//! cadcraft-cli run [FILE|--sample|--metric] [--script TEXT|--script-file F.scr] [--cmd 'id {json}']... [--save OUT] [--export OUT.png]
//! cadcraft-cli commands [FILTER]                   command catalog (JSON)
//! cadcraft-cli mcp [--connect HOST:PORT] [--open FILE]
//!                                                  MCP server on stdio (headless or bridged)
//! cadcraft-cli mcp-config [--ide ID] [--connect HOST:PORT] [--open FILE] [--write]
//!                                                  print/merge the stdio config for an AI IDE
//! cadcraft-cli perf [N]                            timing table on a synthetic N-entity drawing
//! cadcraft-cli sample (bracket|floorplan) OUT       write a built-in sample drawing
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

use cadcraft_engine::Session;
use serde_json::{Value, json};

const USAGE: &str = "usage:
  cadcraft-cli info FILE.dxf
  cadcraft-cli convert IN.dxf OUT.(dxf|svg|png)
  cadcraft-cli run [FILE | --sample | --metric] [--script TEXT] [--script-file F.scr] [--cmd 'id {json}']... [--save OUT.dxf] [--export OUT.(png|svg)]
  cadcraft-cli commands [FILTER]
  cadcraft-cli mcp [--connect HOST:PORT] [--open FILE]
  cadcraft-cli mcp-config [--ide generic|claude-desktop|cursor|trae|windsurf|cline|vscode]
                 [--connect HOST:PORT] [--open FILE] [--write]
  cadcraft-cli perf [N]
  cadcraft-cli sample (bracket|floorplan) OUT.(dxf|dwg|svg|png)
  cadcraft-cli --version";

fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| cadcraft_io::plot(d, space, opts).map_err(|e| e.to_string())),
    });
}

fn open(s: &mut Session, path: &str) -> Result<(), String> {
    s.execute("open", &json!({ "path": path })).map(|_| ()).map_err(|e| e.to_string())
}

fn info(path: &str) -> Result<(), String> {
    let mut s = Session::empty();
    open(&mut s, path)?;
    let v = s.execute("drawing.inspect", &json!({ "entities": false })).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}

fn export(s: &Session, out: &str) -> Result<(), String> {
    let d = s.doc().map_err(|e| e.to_string())?;
    let bytes = cadcraft_io::write(d, out).map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    eprintln!("wrote {out} ({} bytes)", bytes.len());
    Ok(())
}

fn sample(args: &[String]) -> Result<(), String> {
    let (Some(which), Some(out)) = (args.first(), args.get(1)) else { return Err(USAGE.into()) };
    let d = match which.as_str() {
        "bracket" => cadcraft_engine::sample::bracket(),
        "floorplan" | "floor" => cadcraft_engine::sample::floor_plan(),
        other => return Err(format!("unknown sample `{other}` (bracket, floorplan)")),
    };
    let bytes = cadcraft_io::write(&d, out).map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    eprintln!("wrote {out} ({} bytes)", bytes.len());
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let mut s = Session::empty();
    let mut it = args.iter();
    let mut save = None;
    let mut exp = None;
    let mut steps: Vec<(String, String)> = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sample" => {
                s.open_drawing(cadcraft_engine::sample::default_sample(), "Bracket", None);
            }
            "--metric" => {
                s.new_drawing(true);
            }
            "--script" => steps.push(("script".into(), it.next().ok_or("--script needs text")?.replace("\\n", "\n"))),
            "--script-file" => {
                let p = it.next().ok_or("--script-file needs a path")?;
                steps.push(("script".into(), std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?));
            }
            "--cmd" => steps.push(("cmd".into(), it.next().ok_or("--cmd needs `id {json}`")?.clone())),
            "--save" => save = Some(it.next().ok_or("--save needs a path")?.clone()),
            "--export" => exp = Some(it.next().ok_or("--export needs a path")?.clone()),
            f if !f.starts_with("--") => open(&mut s, f)?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if s.docs.is_empty() {
        s.new_drawing(false);
    }
    for (kind, body) in steps {
        match kind.as_str() {
            "script" => s.script(&body).map_err(|e| e.to_string())?,
            _ => {
                let (id, json) = body.split_once(' ').unwrap_or((&body, "{}"));
                let params: Value = serde_json::from_str(json).map_err(|e| format!("--cmd {id}: {e}"))?;
                let r = s.execute(id, &params).map_err(|e| e.to_string())?;
                if !r.is_null() {
                    println!("{r}");
                }
            }
        }
    }
    for l in &s.log {
        eprintln!("{l}");
    }
    if let Some(p) = save {
        export(&s, &p)?;
    }
    if let Some(p) = exp {
        export(&s, &p)?;
    }
    Ok(())
}

/// Small deterministic generator (xorshift64*) so perf runs are repeatable without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// A synthetic drawing with `n` mixed entities (lines, circles, arcs, polylines, text) spread
/// over a square whose side grows with sqrt(n), so density stays constant.
fn synthetic(n: usize, side: f64) -> Result<cadcraft_engine::doc::Drawing, String> {
    use cadcraft_engine::doc::{self as d, Common, EntityKind, Space};
    use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
    let mut dr = d::Drawing::new_imperial();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let v3 = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    for i in 0..n {
        let (x, y) = (r.range(0.0, side), r.range(0.0, side));
        let s = r.range(0.5, 8.0);
        let kind = match i % 5 {
            0 => EntityKind::Line(d::Line { a: v3(x, y), b: v3(x + r.range(-s, s), y + r.range(-s, s)) }),
            1 => EntityKind::Circle(d::Circle { center: v3(x, y), radius: s * 0.5 }),
            2 => EntityKind::Arc(d::Arc { center: v3(x, y), radius: s * 0.5, start: r.range(0.0, 3.0), end: r.range(3.2, 6.2) }),
            3 => EntityKind::LwPolyline(d::LwPolyline {
                vertices: (0..6)
                    .map(|k| PolyVertex {
                        p: Vec2::new(x + f64::from(k) * s * 0.3, y + r.range(-s, s) * 0.3),
                        bulge: if k == 2 { 0.4 } else { 0.0 },
                        start_width: 0.0,
                        end_width: 0.0,
                    })
                    .collect(),
                closed: i % 2 == 0,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
            _ => EntityKind::Text(d::Text {
                insert: v3(x, y),
                align_pt: None,
                height: s * 0.2,
                value: format!("T{i}"),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: d::HAlign::Left,
                valign: d::VAlign::Baseline,
            }),
        };
        dr.add(&Space::Model, Common::default(), kind).map_err(|e| e.to_string())?;
    }
    Ok(dr)
}

fn perf(args: &[String]) -> Result<(), String> {
    use cadcraft_engine::doc::Space;
    use cadcraft_engine::geom::{Bounds2, Vec2};
    use cadcraft_engine::{select, snap};
    use std::time::Instant;
    let n: usize = match args.first() {
        Some(a) => a.parse().map_err(|_| format!("perf: bad entity count {a}"))?,
        None => 200_000,
    };
    let n = n.clamp(1, 20_000_000);
    let side = (n as f64).sqrt() * 10.0;
    let mut rows: Vec<(String, f64, String)> = Vec::new();
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let d = synthetic(n, side)?;
    rows.push(("build synthetic drawing".into(), ms(t), format!("{n} entities")));

    // Display list at a 1600 px wide "zoom extents" view (tolerance = half a pixel).
    let px = side / 1600.0;
    let opts = cadcraft_engine::render::Options { tolerance: px * 0.5, min_dash: px * 2.0, ..Default::default() };
    let t = Instant::now();
    let list = cadcraft_engine::render::build(&d, &Space::Model, &opts);
    rows.push(("display list build".into(), ms(t), format!("{} prims, {} segments", list.prims.len(), list.segment_count())));

    // Hit testing on a zoomed-in view (aperture = 5 px at 0.02 units/px).
    let ap = 0.1;
    let mut r = Rng(42);
    let pts: Vec<Vec2> = (0..1000).map(|_| Vec2::new(r.range(0.0, side), r.range(0.0, side))).collect();
    let t = Instant::now();
    let first = select::pick(&d, &Space::Model, pts.first().copied().unwrap_or(Vec2::ZERO), ap);
    rows.push(("first pick (incl. index build)".into(), ms(t), format!("{first:?}")));
    let t = Instant::now();
    let hits = pts.iter().filter(|p| select::pick(&d, &Space::Model, **p, ap).is_some()).count();
    let total = ms(t);
    rows.push(("pick x1000".into(), total, format!("{hits} hits, {:.3} ms/pick", total / 1000.0)));

    // One edit, then a pick: only the touched chunk is re-indexed.
    let mut edited = d.clone();
    if let Some(h) = edited.model.last().map(|e| e.handle) {
        edited.model.modify(h, |e| e.common.layer = "0".into());
    }
    let t = Instant::now();
    let _ = select::pick(&edited, &Space::Model, pts.first().copied().unwrap_or(Vec2::ZERO), ap);
    rows.push(("pick after one edit (incremental re-index)".into(), ms(t), String::new()));

    let mut sel = 0usize;
    let t = Instant::now();
    for k in 0..20 {
        let c = Vec2::new(r.range(0.0, side), r.range(0.0, side));
        let half = side * 0.05;
        sel += select::select_window(&d, &Space::Model, Bounds2::new(c - Vec2::new(half, half), c + Vec2::new(half, half)), k % 2 == 1).len();
    }
    rows.push(("window/crossing select x20 (10% side)".into(), ms(t), format!("{sel} selected")));

    let t = Instant::now();
    let fence: Vec<Vec2> = (0..4).map(|_| Vec2::new(r.range(0.0, side), r.range(0.0, side))).collect();
    let fsel = select::select_fence(&d, &Space::Model, &fence).len();
    rows.push(("fence select (3 legs)".into(), ms(t), format!("{fsel} selected")));

    let osmode = snap::mode::END | snap::mode::MID | snap::mode::CEN | snap::mode::QUA | snap::mode::INT;
    let t = Instant::now();
    let snaps = pts.iter().filter(|p| snap::osnap(&d, &Space::Model, **p, ap * 2.0, osmode, None).is_some()).count();
    let total = ms(t);
    rows.push(("osnap x1000".into(), total, format!("{snaps} hits, {:.3} ms/query", total / 1000.0)));

    let t = Instant::now();
    let bytes = cadcraft_io::write(&d, "perf.dxf").map_err(|e| e.to_string())?;
    rows.push(("DXF write".into(), ms(t), format!("{:.1} MB", bytes.len() as f64 / 1e6)));
    let t = Instant::now();
    let back = cadcraft_io::read(&bytes, "perf.dxf").map_err(|e| e.to_string())?;
    rows.push(("DXF read".into(), ms(t), format!("{} entities", back.entity_count())));

    println!("{:<40} {:>12}  notes", "operation", "ms");
    println!("{}", "-".repeat(80));
    for (name, t, note) in rows {
        println!("{name:<40} {t:>12.2}  {note}");
    }
    Ok(())
}

/// Pull `--flag VALUE` pairs out of an argument list. Unknown flags / missing values error out.
fn parse_kv_options(args: &[String], allowed: &[&str]) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let key = a.strip_prefix("--").ok_or_else(|| format!("unexpected argument `{a}`"))?;
        if !allowed.contains(&key) {
            return Err(format!("unknown option --{key}"));
        }
        let value = it.next().ok_or_else(|| format!("--{key} needs a value"))?;
        out.insert(key.to_string(), value.clone());
    }
    Ok(out)
}

/// `mcp`: serve MCP over stdio, headless or bridged to a running app.
fn mcp_serve(args: &[String]) -> Result<(), String> {
    let opts = parse_kv_options(args, &["connect", "open"])?;
    // --connect wins; otherwise an env var lets an IDE config launch a bridged server without
    // extra args (CADCRAFT_MCP_CONNECT=127.0.0.1:7979).
    let connect = opts.get("connect").cloned().or_else(|| std::env::var("CADCRAFT_MCP_CONNECT").ok().filter(|a| !a.trim().is_empty()));
    if connect.is_some() && opts.contains_key("open") {
        return Err("--open only applies to the headless backend (without --connect)".into());
    }
    let backend: Box<dyn cadcraft_mcp::Backend> = match connect {
        Some(addr) => Box::new(cadcraft_mcp::Remote::connect(&addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?),
        None => {
            let mut headless = cadcraft_mcp::Headless::default();
            if let Some(path) = opts.get("open") {
                headless.session.execute("open", &json!({ "path": path })).map_err(|e| e.to_string())?;
            }
            Box::new(headless)
        }
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    cadcraft_mcp::Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

/// `mcp-config`: emit (and optionally merge) the stdio config an AI IDE needs to launch us.
fn mcp_config(args: &[String]) -> Result<(), String> {
    use cadcraft_mcp::{Ide, Launch};
    let write = args.iter().any(|a| a == "--write");
    let valued: Vec<String> = args.iter().filter(|a| **a != "--write").cloned().collect();
    let opts = parse_kv_options(&valued, &["ide", "connect", "open"])?;
    let ide = match opts.get("ide") {
        Some(id) => Ide::parse(id)
            .ok_or_else(|| format!("unknown --ide `{id}` (one of: {})", Ide::all().iter().map(|i| i.id()).collect::<Vec<_>>().join(", ")))?,
        None => Ide::Generic,
    };
    let command = std::env::current_exe().ok().and_then(|p| p.to_str().map(str::to_string)).unwrap_or_else(|| "cadcraft-cli".to_string());
    let mut launch = Launch::new(command);
    if let Some(addr) = opts.get("connect") {
        launch = launch.connect(addr);
    }
    if let Some(path) = opts.get("open") {
        launch = launch.open(path);
    }

    let json = cadcraft_mcp::config_snippet(ide, &launch);
    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;

    if write {
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).map_err(|_| "cannot find the home directory (set HOME)")?;
        let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
        let path = ide
            .config_path(std::path::Path::new(&home), &cwd)
            .ok_or_else(|| format!("{} has no conventional config file; rerun without --write and paste the JSON", ide.label()))?;
        let existing = if path.is_file() { std::fs::read_to_string(&path).ok() } else { None };
        let merged = cadcraft_mcp::merge_config(existing.as_deref(), ide, &launch)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, merged).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("merged cadcraft into {} (other servers preserved)", path.display());
    } else {
        eprintln!("{} — paste this into the IDE's MCP settings:", ide.label());
        if let Some(path) = std::env::var("HOME")
            .ok()
            .or_else(|| std::env::var("USERPROFILE").ok())
            .and_then(|h| std::env::current_dir().ok().map(|c| (h, c)))
            .and_then(|(h, c)| ide.config_path(std::path::Path::new(&h), &c))
        {
            eprintln!("(config file: {})", path.display());
        }
    }
    println!("{pretty}");
    Ok(())
}

fn main() -> ExitCode {
    install_io();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let r = match args.first().map(String::as_str) {
        Some("info") => rest.first().ok_or_else(|| USAGE.to_string()).and_then(|p| info(p)),
        Some("convert") => match rest.as_slice() {
            [i, o] => {
                let mut s = Session::empty();
                open(&mut s, i).and_then(|_| export(&s, o))
            }
            _ => Err(USAGE.into()),
        },
        Some("run") => run(&rest),
        Some("perf") => perf(&rest),
        Some("sample") => sample(&rest),
        Some("commands") => {
            let s = Session::new();
            let f = rest.first().map(|x| x.to_ascii_lowercase());
            let v: Vec<Value> = cadcraft_engine::command_specs()
                .iter()
                .filter(|c| f.as_ref().is_none_or(|f| c.id.contains(f.as_str()) || c.label.to_ascii_lowercase().contains(f.as_str())))
                .map(|c| serde_json::to_value(c.info(&s)).unwrap_or_default())
                .collect();
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            Ok(())
        }
        Some("mcp") => mcp_serve(&rest),
        Some("mcp-config") => mcp_config(&rest),
        Some("--version" | "-V") => {
            println!("cadcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(USAGE.into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
