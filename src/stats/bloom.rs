//! Bloom filters for zone-map augmentation: cheap definite negatives so
//! point lookups can skip groups without decoding them.

const WIDTH: usize = 512;

#[derive(Debug, Clone)]
pub struct Bloom {
    bits: [u64; WIDTH / 64],
}

impl Default for Bloom {
    fn default() -> Self {
        Bloom {
            bits: [0; WIDTH / 64],
        }
    }
}

impl Bloom {
    fn hashes(value: &[u8]) -> (usize, usize) {
        let mut h1 = 0xcbf2_9ce4_8422_2325u64;
        let mut h2 = 0x9e37_79b9_7f4a_7c15u64;
        for &b in value {
            h1 ^= b as u64;
            h1 = h1.wrapping_mul(0x0000_0100_0000_01b3);
            h2 ^= (b as u64) << 1;
            h2 = h2.wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
        }
        ((h1 as usize) % WIDTH, (h2 as usize) % WIDTH)
    }

    pub fn add(&mut self, value: &[u8]) {
        let (a, b) = Self::hashes(value);
        self.bits[a / 64] |= 1u64 << (a % 64);
        self.bits[b / 64] |= 1u64 << (b % 64);
    }

    pub fn maybe_contains(&self, value: &[u8]) -> bool {
        let (a, b) = Self::hashes(value);
        self.bits[a / 64] & (1u64 << (a % 64)) != 0 && self.bits[b / 64] & (1u64 << (b % 64)) != 0
    }

    pub fn build(values: &[i64]) -> Self {
        let mut bloom = Bloom::default();
        for v in values {
            bloom.add(&v.to_le_bytes());
        }
        bloom
    }

    pub fn check(&self, value: i64) -> bool {
        self.maybe_contains(&value.to_le_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definite_negatives_and_no_false_negatives() {
        let values: Vec<i64> = (0..100).collect();
        let bloom = Bloom::build(&values);
        for v in &values {
            assert!(bloom.check(*v));
        }
        let mut misses = 0;
        for v in 1000..1100 {
            if !bloom.check(v) {
                misses += 1;
            }
        }
        assert!(misses > 50, "misses {misses}");
    }
}
