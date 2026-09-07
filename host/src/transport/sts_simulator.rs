//! STS拡張プロトコルの模擬機。機構・電気的特性は再現しない。
use std::collections::{BTreeMap, VecDeque};
pub(super) struct StsSimulator {
    regs: BTreeMap<u8, [u8; 71]>,
    staged: BTreeMap<u8, [u8; 8]>,
}
impl Default for StsSimulator {
    fn default() -> Self {
        let mut regs = BTreeMap::new();
        for id in 1..=3 {
            let mut r = [0; 71];
            r[5] = id;
            r[11] = 255;
            r[12] = 15;
            r[55] = 1;
            r[57] = 8;
            r[62] = 120;
            r[63] = 25;
            regs.insert(id, r);
        }
        Self {
            regs,
            staged: BTreeMap::new(),
        }
    }
}
impl StsSimulator {
    pub fn stop(&mut self) {
        for r in self.regs.values_mut() {
            r[40] = 0;
            r[58] = 0;
            r[59] = 0;
        }
        self.staged.clear();
    }
    pub fn handle(&mut self, p: [u8; 8], running: bool, rx: &mut VecDeque<String>) {
        let mut ack = [1, p[1], p[2], p[3], 0, 0, 0, 0];
        match p[1] {
            27 => self.staged.clear(),
            23 => {
                if !running || self.staged.len() != usize::from(p[2]) || self.staged.is_empty() {
                    ack[4] = 1;
                } else {
                    for (id, t) in &self.staged {
                        if let Some(r) = self.regs.get_mut(id) {
                            r[40] = 1;
                            let target = u16::from_be_bytes([t[4], t[5]]);
                            let pos = if r[33] == 3 {
                                let old = crate::application::sts::decode_signed(
                                    u16::from_le_bytes([r[56], r[57]]),
                                    15,
                                );
                                crate::application::sts::signed(
                                    (old + crate::application::sts::decode_signed(target, 15))
                                        .clamp(-28672, 28672)
                                        as i16,
                                )
                            } else {
                                target
                            };
                            r[56..58].copy_from_slice(&pos.to_le_bytes());
                            r[58] = t[7];
                            r[59] = t[6];
                            r[69] = 10;
                        } else {
                            ack[4] = 3;
                        }
                    }
                }
                self.staged.clear();
            }
            22 => {
                if self.regs.contains_key(&p[2]) {
                    self.staged.insert(p[2], p);
                } else {
                    ack[4] = 3;
                }
            }
            25 => {
                if !self.regs.contains_key(&p[2]) {
                    ack[4] = 3;
                }
            }
            20 | 21 | 24 => {
                if let Some(mut r) = self.regs.get(&p[2]).copied() {
                    let address = usize::from(p[4]);
                    let width = usize::from(p[5]);
                    if p[1] == 24 {
                        for part in 0..4 {
                            let mut out = [1, p[2], p[3], part, 0, 0, 0, 0];
                            for i in 0..4 {
                                let n = 56 + usize::from(part) * 4 + i;
                                if n < 71 {
                                    out[4 + i] = r[n];
                                }
                            }
                            emit(807, out, rx);
                        }
                    } else if !(width == 1 || width == 2) || address + width > 71 {
                        ack[4] = 1;
                    } else if p[1] == 20 {
                        ack[5..5 + width].copy_from_slice(&r[address..address + width]);
                    } else if running {
                        ack[4] = 1;
                    } else {
                        r[address] = p[7];
                        if width == 2 {
                            r[address + 1] = p[6];
                        }
                        if address == 33 && p[7] == 3 {
                            r[9..13].fill(0);
                        }
                        r[40] = 0;
                        self.regs.remove(&p[2]);
                        self.regs.insert(r[5], r);
                    }
                } else {
                    ack[4] = 3;
                }
            }
            _ => ack[4] = 1,
        }
        emit(806, ack, rx);
    }
}
fn emit(id: u16, data: [u8; 8], rx: &mut VecDeque<String>) {
    rx.push_back(format!(
        "CAN_RX bus=2 id={id} data={}",
        data.iter().map(|b| format!("{b:02X}")).collect::<String>()
    ));
}
