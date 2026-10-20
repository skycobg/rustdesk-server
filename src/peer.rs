use crate::common::*;
use crate::database;
use hbb_common::{
    bytes::Bytes,
    log,
    rendezvous_proto::*,
    tokio::sync::{Mutex, RwLock},
    ResultType,
};
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, collections::HashSet, net::SocketAddr, sync::Arc, time::Instant};

type IpBlockMap = HashMap<String, ((u32, Instant), (HashSet<String>, Instant))>;
type UserStatusMap = HashMap<Vec<u8>, Arc<(Option<Vec<u8>>, bool)>>;
type IpChangesMap = HashMap<String, (Instant, HashMap<String, i32>)>;
lazy_static::lazy_static! {
    pub(crate) static ref IP_BLOCKER: Mutex<IpBlockMap> = Default::default();
    pub(crate) static ref USER_STATUS: RwLock<UserStatusMap> = Default::default();
    pub(crate) static ref IP_CHANGES: Mutex<IpChangesMap> = Default::default();
}
pub const IP_CHANGE_DUR: u64 = 180;
pub const IP_CHANGE_DUR_X2: u64 = IP_CHANGE_DUR * 2;
pub const DAY_SECONDS: u64 = 3600 * 24;
pub const IP_BLOCK_DUR: u64 = 60;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub(crate) struct PeerInfo {
    #[serde(default)]
    pub(crate) ip: String,
}

pub(crate) struct Peer {
    pub(crate) socket_addr: SocketAddr,
    pub(crate) last_reg_time: Instant,
    pub(crate) guid: Vec<u8>,
    pub(crate) uuid: Bytes,
    pub(crate) pk: Bytes,
    // pub(crate) user: Option<Vec<u8>>,
    pub(crate) info: PeerInfo,
    // pub(crate) disabled: bool,
    pub(crate) reg_pk: (u32, Instant), // how often register_pk
}

impl Default for Peer {
    fn default() -> Self {
        Self {
            socket_addr: "0.0.0.0:0".parse().unwrap(),
            last_reg_time: get_expired_time(),
            guid: Vec::new(),
            uuid: Bytes::new(),
            pk: Bytes::new(),
            info: Default::default(),
            // user: None,
            // disabled: false,
            reg_pk: (0, get_expired_time()),
        }
    }
}

pub(crate) type LockPeer = Arc<RwLock<Peer>>;

#[derive(Clone)]
pub(crate) struct PeerMap {
    map: Arc<RwLock<HashMap<String, LockPeer>>>,
    pub(crate) db: database::Database,
}

impl PeerMap {
    pub(crate) async fn new() -> ResultType<Self> {
        let db = std::env::var("DB_URL").unwrap_or({
            let mut db = "db_v2.sqlite3".to_owned();
            #[cfg(all(windows, not(debug_assertions)))]
            {
                if let Some(path) = hbb_common::config::Config::icon_path().parent() {
                    db = format!("{}\\{}", path.to_str().unwrap_or("."), db);
                }
            }
            #[cfg(not(windows))]
            {
                db = format!("./{db}");
            }
            db
        });
        log::info!("DB_URL={}", db);
        let pm = Self {
            map: Default::default(),
            db: database::Database::new(&db).await?,
        };
        Ok(pm)
    }

    #[inline]
    pub(crate) async fn update_pk(
        &mut self,
        id: String,
        peer: LockPeer,
        addr: SocketAddr,
        uuid: Bytes,
        pk: Bytes,
        ip: String,
    ) -> register_pk_response::Result {
        log::info!("update_pk {} {:?} {:?} {:?}", id, addr, uuid, pk);
        let (info_str, guid) = {
            let mut w = peer.write().await;
            w.socket_addr = addr;
            w.uuid = uuid.clone();
            w.pk = pk.clone();
            w.last_reg_time = Instant::now();
            w.info.ip = ip;
            (
                serde_json::to_string(&w.info).unwrap_or_default(),
                w.guid.clone(),
            )
        };
        if guid.is_empty() {
            match self.db.insert_peer(&id, &uuid, &pk, &info_str).await {
                Err(err) => {
                    log::error!("db.insert_peer failed: {}", err);
                    return register_pk_response::Result::SERVER_ERROR;
                }
                Ok(guid) => {
                    peer.write().await.guid = guid;
                }
            }
        } else {
            if let Err(err) = self.db.update_pk(&guid, &id, &pk, &info_str).await {
                log::error!("db.update_pk failed: {}", err);
                return register_pk_response::Result::SERVER_ERROR;
            }
            log::info!("pk updated instead of insert");
        }
        register_pk_response::Result::OK
    }

    /// SKY: rename the peer `old_id` (already checked by `RendezvousServer::change_id`) to `id`: the same row
    /// (guid), uuid and public key. Done under the map's write lock, so the check that `id` is free and the
    /// rename are one step (the unique index on `id` guards the database too). `old_id` stays reserved for the
    /// same device by a row with its uuid and key: nobody else can register it, and the device may go back.
    pub(crate) async fn change_id(
        &self,
        old_id: &str,
        id: &str,
        peer: LockPeer,
    ) -> register_pk_response::Result {
        use register_pk_response::Result::{ID_EXISTS, OK, SERVER_ERROR, UUID_MISMATCH};
        let (guid, uuid, pk, info_str) = {
            let p = peer.read().await;
            (
                p.guid.clone(),
                p.uuid.clone(),
                p.pk.clone(),
                serde_json::to_string(&p.info).unwrap_or_default(),
            )
        };
        if guid.is_empty() || uuid.is_empty() {
            return UUID_MISMATCH;
        }
        let mut map = self.map.write().await;
        match map.get(old_id) {
            Some(p) if Arc::ptr_eq(p, &peer) => {}
            _ => return UUID_MISMATCH,
        }
        // `id` is free if nobody has it, or it is this device's own reservation.
        let mut own_reservation = None;
        if let Some(other) = map.get(id) {
            let o = other.read().await;
            if !o.uuid.is_empty() && o.uuid != uuid {
                return ID_EXISTS;
            }
        }
        match self.db.get_peer(id).await {
            Ok(Some(row)) => {
                if row.uuid != uuid.to_vec() {
                    return ID_EXISTS;
                }
                own_reservation = Some(row.guid);
            }
            Ok(None) => {}
            Err(err) => {
                log::error!("db.get_peer (change_id {} -> {}) failed: {}", old_id, id, err);
                return SERVER_ERROR;
            }
        }
        if let Some(res_guid) = own_reservation {
            if let Err(err) = self.db.delete_peer(&res_guid).await {
                log::error!("db.delete_peer (change_id {} -> {}) failed: {}", old_id, id, err);
                return SERVER_ERROR;
            }
        }
        if let Err(err) = self.db.update_pk(&guid, id, &pk, &info_str).await {
            log::error!("db.update_pk (change_id {} -> {}) failed: {}", old_id, id, err);
            return SERVER_ERROR;
        }
        if let Err(err) = self.db.insert_peer(old_id, &uuid, &pk, &info_str).await {
            // The rename is done; only the reservation of the old ID is missing.
            log::error!("db.insert_peer (reserve {} after change_id) failed: {}", old_id, err);
        }
        map.remove(old_id);
        map.insert(id.to_owned(), peer);
        OK
    }

    #[inline]
    pub(crate) async fn get(&self, id: &str) -> Option<LockPeer> {
        let p = self.map.read().await.get(id).cloned();
        if p.is_some() {
            return p;
        } else if let Ok(Some(v)) = self.db.get_peer(id).await {
            let peer = Peer {
                guid: v.guid,
                uuid: v.uuid.into(),
                pk: v.pk.into(),
                // user: v.user,
                info: serde_json::from_str::<PeerInfo>(&v.info).unwrap_or_default(),
                // disabled: v.status == Some(0),
                ..Default::default()
            };
            let peer = Arc::new(RwLock::new(peer));
            self.map.write().await.insert(id.to_owned(), peer.clone());
            return Some(peer);
        }
        None
    }

    #[inline]
    pub(crate) async fn get_or(&self, id: &str) -> LockPeer {
        if let Some(p) = self.get(id).await {
            return p;
        }
        let mut w = self.map.write().await;
        if let Some(p) = w.get(id) {
            return p.clone();
        }
        let tmp = LockPeer::default();
        w.insert(id.to_owned(), tmp.clone());
        tmp
    }

    #[inline]
    pub(crate) async fn get_in_memory(&self, id: &str) -> Option<LockPeer> {
        self.map.read().await.get(id).cloned()
    }

    #[inline]
    pub(crate) async fn is_in_memory(&self, id: &str) -> bool {
        self.map.read().await.contains_key(id)
    }
}
