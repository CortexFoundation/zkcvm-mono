use super::*;

// Note: offsets and length params below can be byte or word, or in general
// callee's interpreted
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrecompileCallABI {
    pub input_memory_offset: u32,
    pub input_memory_length: u32,
    pub output_memory_offset: u32,
    pub output_memory_length: u32,
    pub memory_page_to_read: u32,
    pub memory_page_to_write: u32,
    pub precompile_interpreted_data: u64,
}

impl PrecompileCallABI {
    pub const fn from_u256(raw_value: U256) -> Self {
        let raw = raw_value.as_limbs();
        let input_memory_offset = raw[0] as u32;
        let input_memory_length = (raw[0] >> 32) as u32;
        let output_memory_offset = raw[1] as u32;
        let output_memory_length = (raw[1] >> 32) as u32;
        let memory_page_to_read = raw[2] as u32;
        let memory_page_to_write = (raw[2] >> 32) as u32;
        let precompile_interpreted_data = raw[3];

        Self {
            input_memory_offset,
            input_memory_length,
            output_memory_offset,
            output_memory_length,
            memory_page_to_read,
            memory_page_to_write,
            precompile_interpreted_data,
        }
    }

    pub const fn to_u256(self) -> U256 {
        let mut result_limbs = U256::ZERO.into_limbs();
        result_limbs[0] =
            (self.input_memory_offset as u64) | ((self.input_memory_length as u64) << 32);
        result_limbs[1] =
            (self.output_memory_offset as u64) | ((self.output_memory_length as u64) << 32);
        result_limbs[2] =
            (self.memory_page_to_read as u64) | ((self.memory_page_to_write as u64) << 32);
        result_limbs[3] = self.precompile_interpreted_data;

        U256::from_limbs(result_limbs)
    }
}
