// Copyright 2022 CeresDB Project Authors. Licensed under Apache-2.0.

pub mod cache;

use std::sync::Arc;

use common_types::{schema::Schema, time::TimeRange, SequenceNumber};
use common_util::define_result;
use proto::sst as sst_pb;
use snafu::{Backtrace, OptionExt, ResultExt, Snafu};
use table_engine::table::TableId;

use crate::{
    space::SpaceId,
    sst::{
        factory::{FactoryRef, ObjectStorePickerRef, SstReaderOptions},
        file::FileHandle,
        parquet::{
            self, encoding,
            meta_data::{ParquetMetaData, ParquetMetaDataRef},
        },
        reader,
    },
    table::sst_util,
};

/// Error of sst file.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display(
        "Key value metadata in parquet is not found.\nBacktrace\n:{}",
        backtrace
    ))]
    KvMetaDataNotFound { backtrace: Backtrace },

    #[snafu(display("Metadata in proto struct is not found.\nBacktrace\n:{}", backtrace))]
    MetaDataNotFound { backtrace: Backtrace },

    #[snafu(display("Empty custom metadata in parquet.\nBacktrace\n:{}", backtrace))]
    EmptyCustomMetaData { backtrace: Backtrace },

    #[snafu(display("Failed to decode custom metadata in parquet, err:{}", source))]
    DecodeCustomMetaData { source: encoding::Error },

    #[snafu(display("Failed to build sst reader.\nBacktrace:\n:{}", backtrace))]
    BuildSstReader { backtrace: Backtrace },

    #[snafu(display("Failed to read meta data from reader, err:{}", source))]
    ReadMetaData { source: reader::Error },

    #[snafu(display("Failed to convert parquet meta data, err:{}", source))]
    ConvertParquetMetaData { source: parquet::meta_data::Error },
}

define_result!(Error);

#[derive(Debug, Clone, PartialEq)]
pub enum SstMetaData {
    Parquet(ParquetMetaDataRef),
}

impl SstMetaData {
    #[inline]
    pub fn schema(&self) -> &Schema {
        match self {
            Self::Parquet(v) => &v.schema,
        }
    }

    #[inline]
    pub fn time_range(&self) -> TimeRange {
        match self {
            Self::Parquet(v) => v.time_range,
        }
    }

    #[inline]
    pub fn max_sequence(&self) -> SequenceNumber {
        match self {
            Self::Parquet(v) => v.max_sequence,
        }
    }

    #[inline]
    pub fn as_parquet(&self) -> Option<ParquetMetaDataRef> {
        match self {
            Self::Parquet(v) => Some(v.clone()),
        }
    }
}

impl From<SstMetaData> for sst_pb::SstMetaData {
    fn from(src: SstMetaData) -> Self {
        match src {
            SstMetaData::Parquet(meta_data) => {
                let meta_data = sst_pb::ParquetMetaData::from(meta_data.as_ref().to_owned());
                sst_pb::SstMetaData {
                    meta_data: Some(sst_pb::sst_meta_data::MetaData::Parquet(meta_data)),
                }
            }
        }
    }
}

impl TryFrom<sst_pb::SstMetaData> for SstMetaData {
    type Error = Error;

    fn try_from(src: sst_pb::SstMetaData) -> Result<Self> {
        let meta_data = src.meta_data.context(MetaDataNotFound)?;
        match meta_data {
            sst_pb::sst_meta_data::MetaData::Parquet(meta_data) => {
                let parquet_meta_data =
                    ParquetMetaData::try_from(meta_data).context(ConvertParquetMetaData)?;

                Ok(Self::Parquet(Arc::new(parquet_meta_data)))
            }
        }
    }
}

/// A utility reader to fetch meta data of multiple sst files.
pub struct SstMetaReader {
    pub space_id: SpaceId,
    pub table_id: TableId,
    pub factory: FactoryRef,
    pub read_opts: SstReaderOptions,
    pub store_picker: ObjectStorePickerRef,
}

impl SstMetaReader {
    /// Fetch meta data of the `files` from object store.
    pub async fn fetch_metas(&self, files: &[FileHandle]) -> Result<Vec<SstMetaData>> {
        let mut sst_metas = Vec::with_capacity(files.len());
        for f in files {
            let path = sst_util::new_sst_file_path(self.space_id, self.table_id, f.id());
            let mut reader = self
                .factory
                .new_sst_reader(
                    &self.read_opts,
                    &path,
                    f.storage_format(),
                    &self.store_picker,
                )
                .context(BuildSstReader)?;
            let meta_data = reader.meta_data().await.context(ReadMetaData)?;
            sst_metas.push(meta_data.clone());
        }

        Ok(sst_metas)
    }
}

/// Merge multiple sst meta data into the one.
///
/// Panic if the metas is empty.
pub fn merge_sst_meta<'a, I>(metas: I, schema: Schema) -> SstMetaData
where
    I: Iterator<Item = &'a SstMetaData>,
{
    let parquet_metas = metas.map(|v| match v {
        SstMetaData::Parquet(meta_data) => meta_data.as_ref(),
    });

    let parquet_meta_data = parquet::meta_data::merge_sst_meta(parquet_metas, schema);
    SstMetaData::Parquet(Arc::new(parquet_meta_data))
}