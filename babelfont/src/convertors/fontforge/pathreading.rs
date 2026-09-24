use crate::{BabelfontError, FormatSpecific, Node, NodeType, Path};

pub(crate) type SplineSegment = (Vec<(f64, f64)>, char, String);

/// Convert SFD spline lines into a Path structure.
/// Handles contours, segments, and node types.
pub(crate) fn splines_to_path(
    spline_lines: &[String],
    is_quadratic: bool,
) -> Result<Vec<Path>, BabelfontError> {
    let mut paths = Vec::new();
    let mut nodes = Vec::new();
    let mut last_point_flags: Option<String> = None;

    for line in spline_lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Check for contour name/other markers (can be added as format-specific)
        if line.contains(": ") && !line.contains(|c: char| c.is_numeric() || c == '-' || c == '.') {
            // This looks like a key-value (e.g., "Contour name: something")
            // Finish current path if any
            if !nodes.is_empty() {
                paths.push(Path {
                    nodes: nodes.clone(),
                    closed: !is_force_open_path(last_point_flags.as_deref()),
                    ..Default::default()
                });
                nodes.clear();
                last_point_flags = None;
            }
            continue;
        }

        // Try to parse as a segment line
        if let Some((points, seg_type, flags)) = parse_spline_segment(line) {
            let smooth = is_smooth_from_flags(&flags);

            match seg_type {
                'm' => {
                    // Move: start a new contour
                    if !nodes.is_empty() {
                        let mut path = Path {
                            nodes: nodes.clone(),
                            closed: !is_force_open_path(last_point_flags.as_deref()),
                            ..Default::default()
                        };
                        remove_implicit_move_in_closed_path(&mut path);
                        paths.push(path);
                        nodes.clear();
                        last_point_flags = None;
                    }
                    if let Some((x, y)) = points.first() {
                        let mut format_specific = FormatSpecific::default();
                        format_specific.insert(
                            "sfd.point_flags".to_string(),
                            serde_json::Value::String(flags.clone()),
                        );
                        nodes.push(Node {
                            x: *x,
                            y: *y,
                            nodetype: NodeType::Move,
                            smooth,
                            format_specific,
                        });
                        last_point_flags = Some(flags.clone());
                    }
                }
                'l' => {
                    // Line: add a line node
                    if let Some((x, y)) = points.first() {
                        let mut format_specific = FormatSpecific::default();
                        format_specific.insert(
                            "sfd.point_flags".to_string(),
                            serde_json::Value::String(flags.clone()),
                        );
                        nodes.push(Node {
                            x: *x,
                            y: *y,
                            nodetype: NodeType::Line,
                            smooth,
                            format_specific,
                        });
                        last_point_flags = Some(flags.clone());
                    }
                }
                'c' => {
                    if is_quadratic {
                        if let (Some((cx, cy)), Some((x, y))) = (points.first(), points.last()) {
                            nodes.push(Node {
                                x: *cx,
                                y: *cy,
                                nodetype: NodeType::OffCurve,
                                smooth: false,
                                format_specific: Default::default(),
                            });
                            let mut format_specific = FormatSpecific::default();
                            format_specific.insert(
                                "sfd.point_flags".to_string(),
                                serde_json::Value::String(flags.clone()),
                            );
                            nodes.push(Node {
                                x: *x,
                                y: *y,
                                nodetype: NodeType::QCurve,
                                smooth,
                                format_specific,
                            });
                            last_point_flags = Some(flags.clone());
                        }
                    } else {
                        // Cubic curve: add 2 off-curve points, then 1 on-curve
                        for (i, (x, y)) in points.iter().enumerate() {
                            if i < 2 {
                                // Off-curve control points
                                nodes.push(Node {
                                    x: *x,
                                    y: *y,
                                    nodetype: NodeType::OffCurve,
                                    smooth: false,
                                    format_specific: Default::default(),
                                });
                            } else {
                                // Final on-curve point
                                let mut format_specific = FormatSpecific::default();
                                format_specific.insert(
                                    "sfd.point_flags".to_string(),
                                    serde_json::Value::String(flags.clone()),
                                );
                                nodes.push(Node {
                                    x: *x,
                                    y: *y,
                                    nodetype: NodeType::Curve,
                                    smooth,
                                    format_specific,
                                });
                                last_point_flags = Some(flags.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Finish the last path
    if !nodes.is_empty() {
        let mut path = Path {
            nodes: nodes.clone(),
            closed: !is_force_open_path(last_point_flags.as_deref()),
            ..Default::default()
        };
        remove_implicit_move_in_closed_path(&mut path);
        paths.push(path);
    }

    Ok(paths)
}

fn is_force_open_path(flags: Option<&str>) -> bool {
    let Some(raw) = flags else {
        return false;
    };
    let parsed = parse_point_flags(raw).unwrap_or(0);
    (parsed & 0x400) != 0
}

fn parse_point_flags(flags: &str) -> Option<u32> {
    let token = flags
        .split(',')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    if let Some(hex) = token.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        token.parse::<u32>().ok()
    }
}

/// Extract the smooth flag from SFD flags string.
/// Flags are like "0x100,0x200" or just "0". The lower 2 bits encode smoothness.
fn is_smooth_from_flags(flags: &str) -> bool {
    if let Some(part) = flags.split(',').next() {
        if let Some(num_str) = part.strip_prefix("0x") {
            if let Ok(num) = u32::from_str_radix(num_str, 16) {
                return (num & 0x3) != 1;
            }
        } else if let Ok(num) = flags.parse::<u32>() {
            return (num & 0x3) != 1;
        }
    }
    false
}

fn remove_implicit_move_in_closed_path(p: &mut Path) {
    #[allow(clippy::unwrap_used)] // We check for is_empty() before, so unwrap is safe
    if p.closed
        && p.nodes.len() > 1
        && p.nodes.first().map(|n| n.nodetype) == Some(NodeType::Move)
        && p.nodes.first().unwrap().x == p.nodes.last().unwrap().x
        && p.nodes.first().unwrap().y == p.nodes.last().unwrap().y
    {
        p.nodes = p.nodes[1..].to_vec(); // Remove the initial move node if path is closed
    }
}

/// Parse a single spline segment line from SFD format.
/// SFD spline lines have the format: "x1 y1 x2 y2 ... segment_type flags"
/// Where segment_type is 'm' (move), 'l' (line), or 'c' (curve).
/// Returns (points, segment_type, flags).
fn parse_spline_segment(line: &str) -> Option<SplineSegment> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Use regex pattern similar to Python: split on " [lmc] "
    if let Some(m_pos) = line.rfind(" m ") {
        let (coords_str, rest) = line.split_at(m_pos);
        let rest = rest.trim_start_matches(" m ").trim();
        let (flags, _) = rest.split_once(' ').unwrap_or((rest, ""));
        let points = parse_coordinates(coords_str)?;
        return Some((points, 'm', flags.to_string()));
    }
    if let Some(m_pos) = line.rfind(" l ") {
        let (coords_str, rest) = line.split_at(m_pos);
        let rest = rest.trim_start_matches(" l ").trim();
        let (flags, _) = rest.split_once(' ').unwrap_or((rest, ""));
        let points = parse_coordinates(coords_str)?;
        return Some((points, 'l', flags.to_string()));
    }
    if let Some(m_pos) = line.rfind(" c ") {
        let (coords_str, rest) = line.split_at(m_pos);
        let rest = rest.trim_start_matches(" c ").trim();
        let (flags, _) = rest.split_once(' ').unwrap_or((rest, ""));
        let points = parse_coordinates(coords_str)?;
        return Some((points, 'c', flags.to_string()));
    }

    None
}

/// Parse a coordinate string into pairs of (x, y) f64 values.
fn parse_coordinates(coords_str: &str) -> Option<Vec<(f64, f64)>> {
    let values: Result<Vec<f64>, _> = coords_str
        .split_whitespace()
        .map(|s| s.parse::<f64>())
        .collect();

    let values = values.ok()?;
    if values.len() % 2 != 0 {
        return None; // Must have even number of coordinates
    }

    let mut points = Vec::new();
    for chunk in values.chunks(2) {
        if chunk.len() == 2 {
            points.push((chunk[0], chunk[1]));
        }
    }
    Some(points)
}
