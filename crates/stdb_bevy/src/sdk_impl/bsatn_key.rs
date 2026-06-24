use crate::StdbRow;

use super::{Serialize, bsatn};

pub(crate) fn bsatn_key<R: StdbRow + Serialize>(row: &R) -> Vec<u8> {
    bsatn::to_vec(row).unwrap()
}
