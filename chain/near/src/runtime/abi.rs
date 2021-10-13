use crate::codec;
use graph::prelude::BigInt;
use graph::runtime::{asc_new, AscPtr, DeterministicHostError, ToAscObj};
use graph::runtime::{AscHeap, AscIndexId, AscType, IndexForAscTypeId};
use graph::semver;
use graph::{anyhow, semver::Version};
use graph_runtime_derive::AscType;
use graph_runtime_wasm::asc_abi::class::{Array, AscBigInt, Uint8Array};
use std::mem::size_of;

pub(crate) use super::generated::*;

impl ToAscObj<AscBlock> for codec::BlockWrapper {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscBlock, DeterministicHostError> {
        let block = self.block();

        Ok(AscBlock {
            author: asc_new(heap, block.author.as_str())?,
            header: asc_new(heap, self.header())?,
            chunks: asc_new(heap, &block.chunks)?,
        })
    }
}

impl ToAscObj<AscBlockHeader> for codec::BlockHeader {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscBlockHeader, DeterministicHostError> {
        Ok(AscBlockHeader {
            height: self.height,
            prev_height: self.prev_height,
            epoch_id: asc_new(heap, self.epoch_id.as_ref().unwrap())?,
            next_epoch_id: todo!(),
            hash: todo!(),
            prev_hash: todo!(),
            prev_state_root: todo!(),
            chunk_receipts_root: todo!(),
            chunk_headers_root: todo!(),
            chunk_tx_root: todo!(),
            outcome_root: todo!(),
            chunks_included: todo!(),
            challenges_root: todo!(),
            timestamp_nanosec: todo!(),
            random_value: todo!(),
            validator_proposals: todo!(),
            chunk_mask: todo!(),
            gas_price: todo!(),
            block_ordinal: todo!(),
            total_supply: todo!(),
            challenges_result: todo!(),
            last_final_block: todo!(),
            last_ds_final_block: todo!(),
            next_bp_hash: todo!(),
            block_merkle_root: todo!(),
            epoch_sync_data_hash: todo!(),
            approvals: todo!(),
            signature: todo!(),
            latest_protocol_version: todo!(),
        })
    }
}

impl ToAscObj<AscChunkHeader> for codec::ChunkHeader {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscChunkHeader, DeterministicHostError> {
        Ok(AscChunkHeader {
            chunk_hash: asc_new(heap, &Bytes(&self.chunk_hash))?,
            signature: asc_new(heap, self.signature.as_ref().unwrap())?,
            prev_block_hash: asc_new(heap, &Bytes(&self.prev_block_hash))?,
            prev_state_root: todo!(),
            encoded_merkle_root: todo!(),
            encoded_length: todo!(),
            height_created: todo!(),
            height_included: todo!(),
            shard_id: todo!(),
            gas_used: todo!(),
            gas_limit: todo!(),
            balance_burnt: todo!(),
            outgoing_receipts_root: todo!(),
            tx_root: todo!(),
            validator_proposals: todo!(),
        })
    }
}

impl ToAscObj<AscSignature> for codec::Signature {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscSignature, DeterministicHostError> {
        let curve = match self.r#type {
            0 => AscCurveKind::Ed25519,
            1 => AscCurveKind::Secp256K1,
            _ => {
                return Err(DeterministicHostError(anyhow::format_err!(
                    "Invalid signature type value {}",
                    self.r#type
                )))
            }
        };

        Ok(AscSignature {
            kind: asc_new(heap, &curve)?,
            bytes: asc_new(heap, &Bytes(&self.bytes))?,
        })
    }
}

impl ToAscObj<AscChunkHeaderArray> for Vec<codec::ChunkHeader> {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscChunkHeaderArray, DeterministicHostError> {
        let content: Result<Vec<_>, _> = self.iter().map(|x| asc_new(heap, x)).collect();
        let content = content?;
        Ok(AscChunkHeaderArray(Array::new(&*content, heap)?))
    }
}

impl ToAscObj<Uint8Array> for codec::CryptoHash {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscCryptoHash, DeterministicHostError> {
        self.bytes.to_asc_obj(heap)
    }
}

struct Bytes<'a>(&'a Vec<u8>);

impl ToAscObj<Uint8Array> for Bytes<'_> {
    fn to_asc_obj<H: AscHeap + ?Sized>(
        &self,
        heap: &mut H,
    ) -> Result<AscCryptoHash, DeterministicHostError> {
        self.0.to_asc_obj(heap)
    }
}
