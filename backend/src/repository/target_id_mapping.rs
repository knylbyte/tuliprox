use std::cmp::max;
use std::collections::BTreeMap;
use std::io::Error;
use std::path::{Path, PathBuf};

use chrono::Local;
use log::error;
use serde::{Deserialize, Serialize};
use shared::model::{PlaylistItemType, UUIDType};

use crate::model::{DatabaseConfig, PostgresConfig};
use crate::repository::bplustree::BPlusTree;
use postgres::{Client, NoTls};

// TODO make configurable
const EXPIRATION_DURATION: i64 = 86400;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VirtualIdRecord {
    pub virtual_id: u32,
    pub provider_id: u32,
    pub uuid: UUIDType,
    pub item_type: PlaylistItemType,
    pub parent_virtual_id: u32, // only for series to hold series info id.
    pub last_updated: i64,
}

impl VirtualIdRecord {
    fn new(
        provider_id: u32,
        virtual_id: u32,
        item_type: PlaylistItemType,
        parent_virtual_id: u32,
        uuid: UUIDType,
    ) -> Self {
        let last_updated = Local::now().timestamp();
        Self {
            virtual_id,
            provider_id,
            uuid,
            item_type,
            parent_virtual_id,
            last_updated,
        }
    }

    pub fn is_expired(&self) -> bool {
        (Local::now().timestamp() - self.last_updated) > EXPIRATION_DURATION
    }

    pub fn copy_update_timestamp(&self) -> Self {
        Self::new(
            self.provider_id,
            self.virtual_id,
            self.item_type,
            self.parent_virtual_id,
            self.uuid,
        )
    }
}

trait TargetIdMappingStore {
    fn get_and_update_virtual_id(
        &mut self,
        uuid: &UUIDType,
        provider_id: u32,
        item_type: PlaylistItemType,
        parent_virtual_id: u32,
    ) -> u32;
    fn persist(&mut self) -> Result<(), Error>;
}

struct BTreeTargetIdMappingStore {
    dirty: bool,
    virtual_id_counter: u32,
    by_virtual_id: BPlusTree<u32, VirtualIdRecord>,
    by_uuid: BTreeMap<UUIDType, u32>,
    path: PathBuf,
}

impl BTreeTargetIdMappingStore {
    fn new(path: &Path) -> Self {
        let tree_virtual_id: BPlusTree<u32, VirtualIdRecord> =
            BPlusTree::<u32, VirtualIdRecord>::load(path)
                .unwrap_or_else(|_| BPlusTree::<u32, VirtualIdRecord>::new());
        let mut tree_uuid = BTreeMap::new();
        let mut virtual_id_counter: u32 = 0;
        tree_virtual_id.traverse(|keys, values| {
            if let Some(max_value) = keys.iter().max() {
                virtual_id_counter = max(virtual_id_counter, *max_value);
            }
            for v in values {
                tree_uuid.insert(v.uuid, v.virtual_id);
            }
        });
        Self {
            dirty: false,
            virtual_id_counter,
            by_virtual_id: tree_virtual_id,
            by_uuid: tree_uuid,
            path: path.to_path_buf(),
        }
    }
}

impl TargetIdMappingStore for BTreeTargetIdMappingStore {
    fn get_and_update_virtual_id(
        &mut self,
        uuid: &UUIDType,
        provider_id: u32,
        item_type: PlaylistItemType,
        parent_virtual_id: u32,
    ) -> u32 {
        match self.by_uuid.get(uuid) {
            None => {
                self.dirty = true;
                self.virtual_id_counter += 1;
                let virtual_id = self.virtual_id_counter;
                let record = VirtualIdRecord::new(
                    provider_id,
                    virtual_id,
                    item_type,
                    parent_virtual_id,
                    *uuid,
                );
                self.by_virtual_id.insert(virtual_id, record);
                self.by_uuid.insert(*uuid, virtual_id);
                self.virtual_id_counter
            }
            Some(virtual_id) => {
                if let Some(record) = self.by_virtual_id.query(virtual_id) {
                    if record.provider_id == provider_id
                        && (record.item_type != item_type
                            || record.parent_virtual_id != parent_virtual_id)
                    {
                        let new_record = VirtualIdRecord::new(
                            provider_id,
                            *virtual_id,
                            item_type,
                            parent_virtual_id,
                            *uuid,
                        );
                        self.by_virtual_id.insert(*virtual_id, new_record);
                        self.dirty = true;
                    }
                }
                *virtual_id
            }
        }
    }

    fn persist(&mut self) -> Result<(), Error> {
        if self.dirty {
            self.by_virtual_id.store(&self.path)?;
        }
        self.dirty = false;
        Ok(())
    }
}

struct PostgresTargetIdMappingStore {
    client: Client,
}

impl PostgresTargetIdMappingStore {
    fn new(url: &str) -> Result<Self, postgres::Error> {
        let mut client = Client::connect(url, NoTls)?;
        client.batch_execute(
            "CREATE TABLE IF NOT EXISTS target_id_mapping (
                virtual_id SERIAL PRIMARY KEY,
                provider_id INTEGER NOT NULL,
                uuid BYTEA UNIQUE NOT NULL,
                item_type INTEGER NOT NULL,
                parent_virtual_id INTEGER NOT NULL,
                last_updated BIGINT NOT NULL
            )",
        )?;
        Ok(Self { client })
    }
}

impl TargetIdMappingStore for PostgresTargetIdMappingStore {
    fn get_and_update_virtual_id(
        &mut self,
        uuid: &UUIDType,
        provider_id: u32,
        item_type: PlaylistItemType,
        parent_virtual_id: u32,
    ) -> u32 {
        let uuid_bytes: &[u8] = uuid;
        let item_type_i32 = item_type as i32;
        let now = Local::now().timestamp();
        if let Ok(Some(row)) = self.client.query_opt(
            "SELECT virtual_id, provider_id, item_type, parent_virtual_id FROM target_id_mapping WHERE uuid = $1",
            &[&uuid_bytes],
        ) {
            let virtual_id: i32 = row.get(0);
            let stored_provider: i32 = row.get(1);
            let stored_item: i32 = row.get(2);
            let stored_parent: i32 = row.get(3);
            if stored_provider as u32 == provider_id &&
                (stored_item != item_type_i32 || stored_parent as u32 != parent_virtual_id) {
                let _ = self.client.execute(
                    "UPDATE target_id_mapping SET provider_id=$2, item_type=$3, parent_virtual_id=$4, last_updated=$5 WHERE virtual_id=$1",
                    &[&virtual_id, &(provider_id as i32), &item_type_i32, &(parent_virtual_id as i32), &now],
                );
            } else {
                let _ = self.client.execute(
                    "UPDATE target_id_mapping SET last_updated=$2 WHERE virtual_id=$1",
                    &[&virtual_id, &now],
                );
            }
            virtual_id as u32
        } else {
            let row = self.client.query_one(
                "INSERT INTO target_id_mapping (provider_id, uuid, item_type, parent_virtual_id, last_updated) VALUES ($1,$2,$3,$4,$5) RETURNING virtual_id",
                &[&(provider_id as i32), &uuid_bytes, &item_type_i32, &(parent_virtual_id as i32), &now],
            ).expect("failed to insert virtual id record");
            row.get::<_, i32>(0) as u32
        }
    }

    fn persist(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

pub struct TargetIdMapping {
    store: Box<dyn TargetIdMappingStore + Send>,
}

impl TargetIdMapping {
    pub fn new(path: &Path, db: Option<&DatabaseConfig>, pg: Option<&PostgresConfig>) -> Self {
        if let Some(url) = db.and_then(|d| d.url(pg)) {
            let store =
                PostgresTargetIdMappingStore::new(&url).expect("Failed to connect to PostgreSQL");
            Self {
                store: Box::new(store),
            }
        } else {
            let store = BTreeTargetIdMappingStore::new(path);
            Self {
                store: Box::new(store),
            }
        }
    }

    pub fn get_and_update_virtual_id(
        &mut self,
        uuid: &UUIDType,
        provider_id: u32,
        item_type: PlaylistItemType,
        parent_virtual_id: u32,
    ) -> u32 {
        self.store
            .get_and_update_virtual_id(uuid, provider_id, item_type, parent_virtual_id)
    }

    pub fn persist(&mut self) -> Result<(), Error> {
        self.store.persist()
    }
}

impl Drop for TargetIdMapping {
    fn drop(&mut self) {
        if let Err(err) = self.persist() {
            error!("Failed to persist target id mapping {err}");
        }
    }
}
