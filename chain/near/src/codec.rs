#[path = "protobuf/sf.near.codec.v1.rs"]
mod pbcodec;

use graph::{
    blockchain::Block,
    blockchain::BlockPtr,
    prelude::{hex, web3::types::H256, BlockNumber},
};
use std::convert::TryFrom;
use std::fmt::LowerHex;

pub use pbcodec::*;

impl From<&CryptoHash> for H256 {
    fn from(input: &CryptoHash) -> Self {
        H256::from_slice(&input.bytes)
    }
}

impl LowerHex for &CryptoHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(&self.bytes))
    }
}

impl BlockWrapper {
    pub fn block(&self) -> &pbcodec::Block {
        self.block.as_ref().unwrap()
    }

    pub fn header(&self) -> &BlockHeader {
        self.block().header.as_ref().unwrap()
    }

    pub fn parent_ptr(&self) -> Option<BlockPtr> {
        let header = self.header();

        match (header.prev_hash.as_ref(), header.prev_height) {
            (Some(hash), number) => Some(BlockPtr::from((hash.into(), number))),
            _ => None,
        }
    }
}

impl From<BlockWrapper> for BlockPtr {
    fn from(b: BlockWrapper) -> BlockPtr {
        (&b).into()
    }
}

impl<'a> From<&'a BlockWrapper> for BlockPtr {
    fn from(b: &'a BlockWrapper) -> BlockPtr {
        let header = b.header();
        let hash: H256 = header.hash.as_ref().unwrap().into();

        BlockPtr::from((hash, header.height))
    }
}

impl Block for BlockWrapper {
    fn ptr(&self) -> BlockPtr {
        self.into()
    }

    fn parent_ptr(&self) -> Option<BlockPtr> {
        self.parent_ptr()
    }
}

pub trait NearBlockExt {
    fn number(&self) -> BlockNumber;
    fn parent_ptr(&self) -> Option<BlockPtr>;
    fn format(&self) -> String;
    fn block_ptr(&self) -> BlockPtr;
}

impl NearBlockExt for BlockWrapper {
    fn number(&self) -> BlockNumber {
        let header = self.block.as_ref().unwrap().header.as_ref().unwrap();

        BlockNumber::try_from(header.height).unwrap()
    }

    fn parent_ptr(&self) -> Option<BlockPtr> {
        self.parent_ptr()
    }

    fn format(&self) -> String {
        let header = self.block.as_ref().unwrap().header.as_ref().unwrap();

        format!("#{} ({:x})", header.height, header.hash.as_ref().unwrap())
    }

    fn block_ptr(&self) -> BlockPtr {
        self.into()
    }
}
