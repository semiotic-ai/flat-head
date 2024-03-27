use futures::stream::{FuturesOrdered, StreamExt};
use header_accumulator::era_validator::era_validate;
use header_accumulator::types::ExtHeaderRecord;
use sf_protos::ethereum::r#type::v2::Block;

use crate::store;
pub const MAX_EPOCH_SIZE: usize = 8192;
pub const FINAL_EPOCH: usize = 1896;
pub const MERGE_BLOCK: usize = 15537394;

/// verifies flat flies stored in directory against a header accumulator
///
pub async fn verify_eras(
    store_url: &String,
    master_acc_file: Option<&String>,
    start_epoch: usize,
    end_epoch: Option<usize>,
    decompress: Option<bool>,
) -> Result<Vec<usize>, anyhow::Error> {
    let mut validated_epochs = Vec::new();
    for epoch in start_epoch..=end_epoch.unwrap_or(start_epoch + 1) {
        let blocks = get_blocks_from_dir(epoch, store_url, decompress).await?;
        let (successful_headers, _): (Vec<_>, Vec<_>) = blocks
            .iter()
            .cloned()
            .map(|block| ExtHeaderRecord::try_from(&block))
            .fold((Vec::new(), Vec::new()), |(mut succ, mut errs), res| {
                match res {
                    Ok(header) => succ.push(header),
                    Err(e) => {
                        // Log the error or handle it as needed
                        eprintln!("Error converting block: {:?}", e);
                        errs.push(e);
                    }
                };
                (succ, errs)
            });
        let root = era_validate(
            successful_headers,
            master_acc_file,
            epoch,
            Some(epoch + 1),
            None,
        )?;
        validated_epochs.extend(root);
    }

    Ok(validated_epochs)
}

async fn get_blocks_from_dir(
    epoch: usize,
    store_url: &String,
    decompress: Option<bool>,
) -> Result<Vec<Block>, anyhow::Error> {
    let start_100_block = epoch * MAX_EPOCH_SIZE;
    let end_100_block = (epoch + 1) * MAX_EPOCH_SIZE;

    let mut blocks =
        extract_100s_blocks(store_url, start_100_block, end_100_block, decompress).await?;

    if epoch < FINAL_EPOCH {
        blocks = blocks[0..MAX_EPOCH_SIZE].to_vec();
    } else {
        blocks = blocks[0..MERGE_BLOCK].to_vec();
    }

    Ok(blocks)
}

async fn extract_100s_blocks(
    store_url: &String,
    start_block: usize,
    end_block: usize,
    decompress: Option<bool>,
) -> Result<Vec<Block>, anyhow::Error> {
    // Flat files are stored in 100 block files
    // So we need to find the 100 block file that contains the start block and the 100 block file that contains the end block
    let start_100_block = (start_block / 100) * 100;
    let end_100_block = (((end_block as f32) / 100.0).ceil() as usize) * 100;

    let zst_extension = if decompress.unwrap() { ".zst" } else { "" };
    let blocks_store = store::new(store_url).expect("failed to create blocks store");

    let mut futs = FuturesOrdered::new();

    for block_number in (start_100_block..end_100_block).step_by(100) {
        let block_file_name = format!("{:010}.dbin{}", block_number, zst_extension);
        futs.push_back(blocks_store.read_blocks(block_file_name, store::ReadOptions { decompress }))
    }

    let mut all_blocks = Vec::new();

    while let Some(res) = futs.next().await {
        match res {
            Ok(blocks) => all_blocks.extend(blocks),
            Err(e) => println!("{:?}", e),
        }
    }

    // Return only the requested blocks

    Ok(all_blocks[start_block - start_100_block..end_block - start_100_block].to_vec())
}
