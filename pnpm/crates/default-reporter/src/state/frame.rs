/// Lazily-assigned block indices for one logical output stream — its
/// non-fixed (`block`) and fixed (`fixed`) slots in the frame — the
/// per-stream `currentBlockNo` / `currentFixedBlockNo` pair.
#[derive(Debug, Default, Clone)]
pub(super) struct BlockSlot {
    pub(super) block: Option<usize>,
    pub(super) fixed: Option<usize>,
}

/// The frame buffer: scrolling `blocks` rendered above pinned `fixed_blocks`,
/// or — in append-only mode — a list of `pending` lines to print as they
/// arrive.
#[derive(Debug)]
pub(super) struct Frame {
    pub(super) append_only: bool,
    pub(super) blocks: Vec<Option<String>>,
    pub(super) fixed_blocks: Vec<Option<String>>,
    pub(super) next_block: usize,
    pub(super) next_fixed: usize,
    pub(super) pending: Vec<String>,
}

impl Frame {
    pub(super) fn new(append_only: bool) -> Self {
        Frame {
            append_only,
            blocks: Vec::new(),
            fixed_blocks: Vec::new(),
            next_block: 0,
            next_fixed: 0,
            pending: Vec::new(),
        }
    }

    pub(super) fn emit(&mut self, slot: &mut BlockSlot, msg: String, fixed: bool) {
        if self.append_only {
            self.pending.push(msg);
            return;
        }
        if fixed {
            let idx = *slot.fixed.get_or_insert_with(|| {
                let assigned = self.next_fixed;
                self.next_fixed += 1;
                assigned
            });
            if self.fixed_blocks.len() <= idx {
                self.fixed_blocks.resize(idx + 1, None);
            }
            self.fixed_blocks[idx] = Some(msg);
        } else {
            if let Some(f) = slot.fixed.take() {
                self.fixed_blocks[f] = None;
            }
            let idx = *slot.block.get_or_insert_with(|| {
                let assigned = self.next_block;
                self.next_block += 1;
                assigned
            });
            if self.blocks.len() <= idx {
                self.blocks.resize(idx + 1, None);
            }
            self.blocks[idx] = Some(msg);
        }
    }

    pub(super) fn render(&self) -> String {
        let non_fixed: Vec<&str> = self.blocks.iter().filter_map(|b| b.as_deref()).collect();
        let fixed: Vec<&str> = self.fixed_blocks.iter().filter_map(|b| b.as_deref()).collect();
        let non_fixed_part = non_fixed.join("\n");
        if fixed.is_empty() {
            return non_fixed_part;
        }
        let fixed_part = fixed.join("\n");
        if non_fixed_part.is_empty() {
            return fixed_part;
        }
        format!("{non_fixed_part}\n{fixed_part}")
    }
}
