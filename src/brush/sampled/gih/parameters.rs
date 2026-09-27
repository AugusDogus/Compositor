use super::{MAX_CELLS, Result, Selection, invalid, selection::Dimension};
use std::collections::HashMap;

pub(super) fn parse(bytes: &[u8]) -> Result<(usize, Vec<Dimension>)> {
    let invalid_header = || {
        invalid(
            "The GIH brush has invalid cell counts or dimensions. Export it again from GIMP; no brush was loaded.",
        )
    };
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_header())?;
    let mut tokens = text.split_whitespace();
    let count = tokens
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| (1..=MAX_CELLS).contains(v))
        .ok_or_else(invalid_header)?;
    let mut params = HashMap::new();
    for token in tokens {
        let (key, value) = token.split_once(':').ok_or_else(invalid_header)?;
        if key.is_empty() || value.is_empty() || params.insert(key, value).is_some() {
            return Err(invalid_header());
        }
    }
    if params.is_empty() {
        return Ok((
            count,
            vec![Dimension {
                rank: count as u32,
                stride: 1,
                selection: Selection::Incremental,
            }],
        ));
    }
    let number = |key: &str, default: u32| -> Result<u32> {
        params
            .get(key)
            .map_or(Ok(default), |v| v.parse().map_err(|_| invalid_header()))
    };
    let dim = number("dim", 1)?;
    if !(1..=4).contains(&dim) {
        return Err(invalid_header());
    }
    let mut dimensions = Vec::new();
    let mut total = 1u32;
    for index in 0..dim {
        let rank = number(&format!("rank{index}"), 1)?;
        if !(1..=MAX_CELLS as u32).contains(&rank) {
            return Err(invalid_header());
        }
        total = total.checked_mul(rank).ok_or_else(invalid_header)?;
        let selection = params
            .get(format!("sel{index}").as_str())
            .copied()
            .or_else(|| {
                (index == 0)
                    .then(|| params.get("selection").copied())
                    .flatten()
            })
            .unwrap_or("random");
        let selection = match selection {
            "constant" => Selection::Constant,
            "incremental" => Selection::Incremental,
            "random" => Selection::Random,
            "angular" => Selection::Angular,
            "pressure" => Selection::Pressure,
            "xtilt" => Selection::XTilt,
            "ytilt" => Selection::YTilt,
            "velocity" => {
                return Err(invalid(
                    "Velocity-selected GIH brushes are not supported. Export a copy using incremental, random, angular, pressure or tilt selection; no brush was loaded.",
                ));
            }
            other => {
                return Err(invalid(format!(
                    "GIH selection mode '{other}' is not supported. Export a copy with a supported selection mode; no brush was loaded."
                )));
            }
        };
        dimensions.push(Dimension {
            rank,
            stride: 0,
            selection,
        });
    }
    for dimension in &mut dimensions {
        total /= dimension.rank;
        dimension.stride = total;
    }
    Ok((count, dimensions))
}
