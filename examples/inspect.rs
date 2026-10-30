//! Prints objects and topology stats for model files, like analisis/scripts/stlstats.py.
//! Usage: cargo run --release --example inspect -- [--repair] file...

use buildcraft3d::{io, repair};

fn main() -> anyhow::Result<()> {
    let do_repair = std::env::args().any(|a| a == "--repair");
    for arg in std::env::args().skip(1).filter(|a| a != "--repair") {
        let m = io::load(std::path::Path::new(&arg))?;
        println!("{arg} (units known: {})", m.units_known);
        for o in &m.objects {
            let fixed;
            let o = if do_repair {
                let (mesh, report) = repair::repair(&o.mesh);
                println!("  repair: {report:?}");
                fixed = io::ModelObject { mesh, ..o.clone() };
                &fixed
            } else {
                o
            };
            let s = o.mesh.stats();
            let b = o.mesh.aabb_transformed(&o.transform);
            println!(
                "  {:<24} tris {:>8} verts {:>8} boundary {:>4} nonmanifold {:>4} misoriented {:>4} comps {:>3} vol {:>12.2} color #{:02X}{:02X}{:02X} bbox {:?}..{:?}",
                o.name, s.triangles, s.vertices, s.boundary_edges, s.nonmanifold_edges, s.misoriented_edges, s.components, s.volume,
                o.color[0], o.color[1], o.color[2], b.min.to_array().map(|v| (v * 100.0).round() / 100.0), b.max.to_array().map(|v| (v * 100.0).round() / 100.0)
            );
        }
    }
    Ok(())
}
