use super::{Tip, gih};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(in crate::brush) enum Source {
    Tip(Arc<Tip>),
    Hose(Arc<gih::Hose>),
}
impl Source {
    pub fn cells(&self) -> &[Arc<Tip>] {
        match self {
            Self::Tip(tip) => std::slice::from_ref(tip),
            Self::Hose(hose) => hose.cells(),
        }
    }
    pub fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Tip(a), Self::Tip(b)) => Arc::ptr_eq(a, b),
            (Self::Hose(a), Self::Hose(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    /// Four-word cell headers followed by word-aligned, packed coverage bytes.
    pub fn gpu_bytes(&self) -> Vec<u8> {
        let cells = self.cells();
        let mut bytes = vec![0; cells.len() * 16];
        for (index, cell) in cells.iter().enumerate() {
            let header = [
                cell.pixels().width(),
                cell.pixels().height(),
                bytes.len() as u32,
                0,
            ];
            for (slot, value) in bytes[index * 16..index * 16 + 16]
                .chunks_exact_mut(4)
                .zip(header)
            {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(cell.pixels().as_raw());
            bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        }
        bytes
    }
}

pub(in crate::brush) struct Stamp {
    pub source: Source,
    pub plan: [[u32; 4]; 4],
    pub dab: u32,
    pub seed: u32,
}
impl Stamp {
    pub fn cell(&self, relative: u32) -> &Tip {
        let cells = self.source.cells();
        if cells.len() == 1 {
            return &cells[0];
        }
        let index = gih::selection::select(self.plan, self.dab.wrapping_add(relative), self.seed);
        &cells[index.min(cells.len() - 1)]
    }
}
