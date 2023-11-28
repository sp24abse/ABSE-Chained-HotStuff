use crate::{
  crypto::{Digest, PublicKey, Signature},
  data::{BlockType, QC},
  node_config::NodeConfig,
  abse::ABSE,
};
use std::{
  collections::{BTreeMap, HashMap},
  slice::Iter,
  sync::{
      atomic::{AtomicU64, Ordering},
      Arc,
  },
};

use tokio::sync::{mpsc::Sender, Notify};

use serde::{Deserialize, Serialize};

use parking_lot::Mutex;
use tracing::{debug, trace, warn};

use crate::{
  data::{Block, BlockTree},
  network::MemoryNetworkAdaptor,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum Message {
  Propose(Block),
  ProposeInBetween(Block),
  Vote(Digest, PublicKey, Signature),
  // Contain the last vote of the sender, so that
  // it can tolerate more failures.
  NewView(QC, Digest, PublicKey, Signature),
}

impl Message {}

pub(crate) struct VoterState {
  pub id: PublicKey,
  pub view: u64,
  pub view_cp: u64,
  pub last_view: i32,
  maxjmp: usize,
  pub threshold: usize,
  pub generic_qc: QC,
  pub locked_qc: QC,
  // <view, (what, whos)>
  pub votes: HashMap<u64, HashMap<Digest, Vec<PublicKey>>>,
  pub notify: Arc<Notify>,
  pub best_view: Arc<AtomicU64>,
  // <view, (whos)>
  pub new_views: HashMap<u64, Vec<PublicKey>>,
  pub abse_struct: ABSE,
  pub id_to_index: HashMap<PublicKey, usize>,
  pub score_array: Vec<u64>,
}

impl VoterState {
  pub fn new(id: PublicKey, view: u64, maxjmp: usize, generic_qc: QC, threshold: usize, abse_struct: ABSE, id_to_index: HashMap<PublicKey, usize>, lenn: usize) -> Self {
    let score_array = vec![0; lenn];
      Self {
          id,
          view_cp: view.clone(),
          last_view: view.clone() as i32 -1,
          view,
          maxjmp,
          threshold,
          generic_qc: generic_qc.to_owned(),
          locked_qc: generic_qc,
          votes: HashMap::new(),
          notify: Arc::new(Notify::new()),
          best_view: Arc::new(AtomicU64::new(0)),
          new_views: HashMap::new(),
          abse_struct,
          id_to_index,
          score_array,
      }
  }

  pub(crate) fn get_index(&self, voter_id: PublicKey) -> Option<usize> {
    self.id_to_index.get(&voter_id).cloned()
  }

  pub(crate) fn reset_array(&mut self) { // 新增这个函数
    for item in &mut self.score_array {
        *item = 0;
    }
  }

  pub fn get_array(&self) -> &[u64] {
    &self.score_array
  }

  pub(crate) fn set_voter_id(&mut self, voter_id: PublicKey) {
    trace!("Set for: {}!",voter_id);
    if let Some(index) = self.get_index(voter_id) {
        self.score_array[index] = 1;
        trace!("Success! Current array is: {:?}", self.score_array);
    }
  }


  pub(crate) fn view_add_multi(&mut self, multiview: u64) {
      // println!("{}: view add to {}", self.id, self.view + 1);
      // Prune old votes
      self.votes.retain(|v, _| v >= &self.view);
      self.new_views.retain(|v, _| v >= &self.view);

      self.view_cp = multiview;
      self.last_view += 1;
      //self.notify.notify_waiters();
  }

  pub(crate) fn multi_retain(&mut self) {
    self.votes.retain(|v, _| v >= &self.view);
    self.new_views.retain(|v, _| v >= &self.view);
}

  pub(crate) fn success_jmp(&mut self) {
  self.view = self.view_cp.clone();
  self.last_view = self.view_cp.clone() as i32 - 1;
}

  pub(crate) fn fail_jmp(&mut self) {
  self.view_cp = self.view.clone();
  }

  pub(crate) fn view_add_one(&mut self) {
    // println!("{}: view add to {}", self.id, self.view + 1);
    // Prune old votes
    self.votes.retain(|v, _| v >= &self.view);
    self.new_views.retain(|v, _| v >= &self.view);

    self.view += 1;
    self.last_view += 1;
    self.view_cp += 1;
    trace!("Attemping trigger waiters...");
    self.notify.notify_waiters();
    trace!("Success triggering!");
}

  pub(crate) fn add_new_view(&mut self, view: u64, who: PublicKey) {
      let view_map = self.new_views.entry(view).or_default();
      // TODO, use a hashmap to collect messages.
      view_map.push(who);

      if view_map.len() == self.threshold {
          trace!(
              "{}: new view {} is ready, current: {}",
              self.id,
              view,
              self.view
          );
          self.best_view.store(view, Ordering::SeqCst);
          self.notify.notify_waiters();
      }
  }

  // return whether a new qc formed.
  pub(crate) fn add_vote(
      &mut self,
      msg_view: u64,
      block_hash: Digest,
      voter_id: PublicKey,
  ) -> Option<QC> {
      self.set_voter_id(voter_id.clone());
      let view_map = self.votes.entry(msg_view).or_default();
      let voters = view_map.entry(block_hash).or_default();
      // TODO: check if voter_id is already in voters
      voters.push(voter_id);
      

      if voters.len() == self.threshold {
          trace!(
              "{}: Vote threshould {} is ready, current: {}",
              self.id,
              msg_view,
              self.view
          );
          let s_array = self.get_array().to_vec();
          //trace!("Set info: {:?}!",s_array);
          trace!("Set info!");
          self.abse_struct.set_info(s_array);
          self.reset_array();
          Some(QC::new(block_hash, msg_view))
      } else {
          trace!(
              "{}: Vote threshould {} is not ready, current: {}, threadhold: {}",
              self.id,
              msg_view,
              self.view,
              self.threshold
          );
          None
      }
  }

  pub(crate) fn set_best_view(&mut self, view: u64) {
      self.best_view.store(view, Ordering::Relaxed);
  }

  pub(crate) fn best_view_ref(&self) -> Arc<AtomicU64> {
      self.best_view.to_owned()
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkPackage {
  pub(crate) from: PublicKey,
  /// None means the message is a broadcast message.
  pub(crate) to: Option<PublicKey>,
  /// None means the message is a global message.
  pub(crate) view: Option<u64>,
  pub(crate) message: Message,
  pub(crate) signature: u64,
}

pub struct Environment {
  pub(crate) block_tree: BlockTree,
  voter_set: VoterSet,
  network: MemoryNetworkAdaptor,
  pub(crate) finalized_block_tx: Option<Sender<(Block, BlockType, u64)>>,
}

impl Environment {
  pub(crate) fn new(
      block_tree: BlockTree,
      voter_set: VoterSet,
      network: MemoryNetworkAdaptor,
  ) -> Self {
      Self {
          block_tree,
          voter_set,
          network,
          finalized_block_tx: None,
      }
  }

  pub(crate) fn register_finalized_block_tx(&mut self, tx: Sender<(Block, BlockType, u64)>) {
      self.finalized_block_tx = Some(tx);
  }
}

pub(crate) struct Voter {
  id: PublicKey,
  config: NodeConfig,
  /// Only used when initialize ConsensusVoter.
  view: u64,
  env: Arc<Mutex<Environment>>,
}

#[derive(Debug, Clone)]
pub(crate) struct VoterSet {
  voters: Vec<PublicKey>,
  id_to_index: HashMap<PublicKey, usize>,
}

impl VoterSet {
  pub fn new(voters: Vec<PublicKey>) -> Self {
    let mut id_to_index = HashMap::new();
    for (i, voter) in voters.iter().enumerate() {
        id_to_index.insert(*voter, i);
    }
    Self { voters, id_to_index} 
  }

  pub fn threshold(&self) -> usize {
      self.voters.len() - (self.voters.len() as f64 / 3.0).floor() as usize
  }

  pub fn iter(&self) -> Iter<PublicKey> {
      self.voters.iter()
  }

  pub fn get_index(&self, voter_id: PublicKey) -> Option<usize> {
    self.id_to_index.get(&voter_id).cloned()
}
}

impl Iterator for VoterSet {
  type Item = PublicKey;

  fn next(&mut self) -> Option<Self::Item> {
      self.voters.pop()
  }
}

impl Voter {
  pub(crate) fn new(id: PublicKey, config: NodeConfig, env: Arc<Mutex<Environment>>) -> Self {
      let view = 1;
      Self {
          id,
          config,
          view,
          env,
      }
  }

  pub(crate) async fn start(&mut self) {
      // Start from view 0, and keep increasing the view number
      let generic_qc = self.env.lock().block_tree.genesis().0.justify.clone();
      let voters = self.env.lock().voter_set.to_owned();
      let id_to_index = voters.id_to_index.clone();
      //trace!("voters: {:?}", voters);
      let lenn = voters.voters.len().clone();
      let faulties = lenn - voters.threshold();
      let state = Arc::new(Mutex::new(VoterState::new(
          self.id,
          self.view,
          self.config.get_node_settings().maxjmp,
          generic_qc,
          voters.threshold(),
          ABSE::new(2, faulties as u64),
          id_to_index,
          lenn,
      )));
      let notify = state.lock().best_view_ref();
      let leadership = Leadership::new(voters, self.config.get_node_settings().leader_rotation);

      let voter = ConsensusVoter::new(
          self.config.to_owned(),
          leadership,
          state.to_owned(),
          self.env.to_owned(),
          notify.to_owned(),
      );
      let leader = voter.clone();
      let pacemaker = voter.clone();

      let handler1 = tokio::spawn(async {
          leader.run_as_leader().await;
      });

      let handler2 = tokio::spawn(async {
          voter.run_as_voter().await;
      });
      
      if self.config.get_node_settings().pretend_failure{
        let (r1, r2) = tokio::join!(handler1, handler2);
        // TODO: handle error
        r1.unwrap();
        r2.unwrap();
      }else{
        let handler3 = tokio::spawn(async {
          pacemaker.run_as_pacemaker().await;
        });
        let (r1, r2, r3) = tokio::join!(handler1, handler2, handler3);
      // TODO: handle error
      r1.unwrap();
      r2.unwrap();
      r3.unwrap();
      }

      // let handler3 = tokio::spawn(async {
      //     pacemaker.run_as_pacemaker().await;
      // });

      // let (r1, r2, r3) = tokio::join!(handler1, handler2, handler3);
      // // TODO: handle error
      // r1.unwrap();
      // r2.unwrap();
      // r3.unwrap();
  }
}

#[derive(Clone)]
struct ConsensusVoter {
  config: NodeConfig,
  leadership: Leadership,
  state: Arc<Mutex<VoterState>>,
  env: Arc<Mutex<Environment>>,
  collect_view: Arc<AtomicU64>,
}

#[derive(Clone)]
struct Leadership {
  voters: VoterSet,
  leader_rotation: usize,
}

impl Leadership {
  fn new(voters: VoterSet, leader_rotation: usize) -> Self {
      Self {
          voters,
          leader_rotation,
      }
  }

  fn get_leader(&self, view: u64) -> PublicKey {
      self.voters
          .voters
          .get(((view / self.leader_rotation as u64) % self.voters.voters.len() as u64) as usize)
          .unwrap()
          .to_owned()
  }
}

impl ConsensusVoter {
  fn new(
      config: NodeConfig,
      leadership: Leadership,
      state: Arc<Mutex<VoterState>>,
      env: Arc<Mutex<Environment>>,
      collect_view: Arc<AtomicU64>,
  ) -> Self {
      Self {
          config,
          state,
          env,
          collect_view,
          leadership,
      }
  }

  // fn get_leader(view: u64, voters: &VoterSet, leader_rotation: usize) -> PublicKey {
  //     voters
  //         .voters
  //         .get(((view / leader_rotation as u64) % voters.voters.len() as u64) as usize)
  //         .unwrap()
  //         .to_owned()
  // }

  fn new_in_between_block(
      env: Arc<Mutex<Environment>>,
      view: u64,
      generic_qc: QC,
      id: PublicKey,
      lower_bound: usize,
  ) -> Option<NetworkPackage> {
      env.lock()
          .block_tree
          .new_in_between_block(generic_qc, lower_bound)
          .map(|blk| Self::package_message(id, Message::ProposeInBetween(blk), view, None))
  }

  fn package_message(
      id: PublicKey,
      message: Message,
      view: u64,
      to: Option<PublicKey>,
  ) -> NetworkPackage {
      NetworkPackage {
          from: id,
          to,
          view: Some(view),
          message,
          signature: 0,
      }
  }

  fn new_key_block(
      env: Arc<Mutex<Environment>>,
      view: u64,
      generic_qc: QC,
      id: PublicKey,
  ) -> NetworkPackage {
      let block = env.lock().block_tree.new_key_block(generic_qc);
      Self::package_message(id, Message::Propose(block), view, None)
  }

  fn update_qc_high(&self, new_qc: QC) -> bool {
      let mut state = self.state.lock();
      if new_qc.view > state.generic_qc.view {
          debug!(
              "Node {} update qc_high from {:?} to {:?}",
              self.config.get_id(),
              state.generic_qc,
              new_qc
          );
          state.generic_qc = new_qc.to_owned();
          drop(state);
          self.env
              .lock()
              .block_tree
              .switch_latest_key_block(new_qc.node);
          true
      } else {
          false
      }
  }

  async fn process_message(
      &mut self,
      pkg: NetworkPackage,
      id: PublicKey,
      voted_view: &mut u64,
      tx: &Sender<NetworkPackage>,
      finalized_block_tx: &Option<Sender<(Block, BlockType, u64)>>,
  ) {
      let view = pkg.view.unwrap();
      let message = pkg.message;
      let from = pkg.from;
      let current_view = self.state.lock().view;
      match message {
          Message::Propose(block) => {
              if view < self.state.lock().view {
                  return;
              }
              let hash = block.hash();

              // WARN: As a POC, we suppose all the blocks are valid by application logic.
              block.verify().unwrap();

              let b_x = block.justify.node;
              let block_justify = block.justify.clone();
              let block_hash = block.hash();
              //trace!("receive block from: {}, view:{}, current view:{}",from, view, current_view);

              if from != id {
                  self.env.lock().block_tree.add_block(block, BlockType::Key);
              }
              let mut jmp = 1;
              // onReceiveProposal
              if let Some(pkg) = {
                  let locked_qc = self.state.lock().locked_qc.clone();
                  let safety = self
                      .env
                      .lock()
                      .block_tree
                      .extends(locked_qc.node, block_hash);
                  let liveness = block_justify.view >= locked_qc.view;

                  if view > *voted_view && (safety || liveness) {
                    if from != id{
                      self.state.lock().set_voter_id(from.clone());
                      let s_array = self.state.lock().get_array().to_vec();
                      //trace!("Set info: {:?}!",s_array);
                      trace!("Set info for leader!");
                      self.state.lock().abse_struct.set_info(s_array);
                      self.state.lock().reset_array();
                    }
                      //self.state.lock().set_voter_id(from.clone());
                      *voted_view = view;
                      let mjmp = self.state.lock().maxjmp.clone() as u64;
                      let abse_s = self.state.lock().abse_struct.clone();
                      trace!("ABSE: {:?}", abse_s);
                      for i in 1..=mjmp{
                        if let Some(index) = self.state.lock().get_index(self.leadership.get_leader(current_view + i)){
                          if abse_s.judge(index) {
                            trace!("{}-{}: can be the leader of view {}", index, self.leadership.get_leader(current_view + i), current_view + i);
                            jmp = i;
                            break;
                          }else {
                            trace!("{}-{}: can not be the leader of view {}, prepare view jump.", index, self.leadership.get_leader(current_view + i), current_view + i);
                          }
                        }
                    }
                    if jmp > 1 {
                      trace!("Target is view {}.", current_view + jmp);
                      Some(Self::package_message(
                        id,
                        Message::Vote(hash, id, self.config.sign(&hash)),
                        current_view + jmp -1,
                        Some(self.leadership.get_leader(current_view + jmp)),
                    ))
                    }else{
                        // Suppose the block is valid, vote for it
                        Some(Self::package_message(
                        id,
                        Message::Vote(hash, id, self.config.sign(&hash)),
                        current_view,
                        Some(self.leadership.get_leader(current_view + 1)),
                    )) 
                    }
                        // Suppose the block is valid, vote for it
                    //     Some(Self::package_message(
                    //     id,
                    //     Message::Vote(hash, id, self.config.sign(&hash)),
                    //     current_view,
                    //     Some(self.leadership.get_leader(current_view + 1)),
                    // )) 
                  } else {
                      trace!(
                          "{}: Safety: {} or Liveness: {} are both invalid",
                          id,
                          safety,
                          liveness
                      );
                      None
                  }
              } {
                  trace!("{}: send vote {:?} for block", id, pkg);
                  tx.send(pkg).await.unwrap();
              }

              // update
              let b_y = self
                  .env
                  .lock()
                  .block_tree
                  .get_block(b_x)
                  .unwrap()
                  .0
                  .justify
                  .node;
              let b_z = self
                  .env
                  .lock()
                  .block_tree
                  .get_block(b_y)
                  .unwrap()
                  .0
                  .justify
                  .node;

              trace!("{}: enter PRE-COMMIT phase", id);
              // PRE-COMMIT phase on b_x
              self.update_qc_high(block_justify);

              let larger_view = self
                  .env
                  .lock()
                  .block_tree
                  .get_block(b_x)
                  .unwrap()
                  .0
                  .justify
                  .view
                  > self.state.lock().locked_qc.view;
              if larger_view {
                  trace!("{}: enter COMMIT phase", id);
                  // COMMIT phase on b_y
                  self.state.lock().locked_qc = self
                      .env
                      .lock()
                      .block_tree
                      .get_block(b_x)
                      .unwrap()
                      .0
                      .justify
                      .clone();
              }

              let is_parent = self.env.lock().block_tree.is_parent(b_y, b_x);
              if is_parent {
                  let is_parent = self.env.lock().block_tree.is_parent(b_z, b_y);
                  if is_parent {
                      trace!("{}: enter DECIDE phase", id);
                      // DECIDE phase on b_z / Finalize b_z
                      let finalized_blocks = self.env.lock().block_tree.finalize(b_z);
                      // onCommit
                      if let Some(tx) = finalized_block_tx.as_ref() {
                          for block in finalized_blocks {
                              tx.send(block).await.unwrap();
                          }
                      }
                  }
              }
              if jmp > 1{
                trace!("{}: view add multi", id);
              // Finish the view
                self.state.lock().view_add_multi(current_view + jmp);
              }else{
                trace!("{}: view add one", id);
                // Finish the view
                self.state.lock().view_add_one();
              }
              // trace!("{}: view add one", id);
              // // Finish the view
              // self.state.lock().view_add_one();
              tracing::trace!("{}: voter finish view: {}", id, current_view);
          }
          Message::ProposeInBetween(block) => {
              let _hash = block.hash();

              block.verify().unwrap();

              if from != id {
                  // Add block to block tree
                  self.env
                      .lock()
                      .block_tree
                      .add_block(block, BlockType::InBetween);
              }
          }
          Message::Vote(block_hash, author, signature) => {
              // onReceiveVote
              // verify signature
              author.verify(&block_hash, &signature).unwrap();
              //self.state.lock().set_voter_id(from);
              let qc = self.state.lock().add_vote(view, block_hash, from);

              if let Some(qc) = qc {
                  self.update_qc_high(qc);
                  self.state.lock().set_best_view(view);
                  
              }
          }
          Message::NewView(high_qc, digest, author, signature) => {
              self.update_qc_high(high_qc);

              author.verify(&digest, &signature).unwrap();
              //self.state.lock().set_voter_id(from);
              let qc = self.state.lock().add_vote(view, digest, from);

              if let Some(qc) = qc {
                  self.update_qc_high(qc);
                  self.state.lock().set_best_view(view);
              }

              self.state.lock().add_new_view(view, from);
          }
      }
  }

  async fn run_as_voter(mut self) {
      let id = self.state.lock().id;
      let finalized_block_tx = self.env.lock().finalized_block_tx.to_owned();
      let (mut rx, tx) = {
          let mut env = self.env.lock();
          let rx = env.network.take_receiver();
          let tx = env.network.get_sender();
          (rx, tx)
      };
      let mut buffer: BTreeMap<u64, Vec<NetworkPackage>> = BTreeMap::new();

      // The view voted for last block.
      //
      // Initialize as 0, since we voted for genesis block.
      let mut voted_view = 0;

      while let Some(pkg) = rx.recv().await {
          let view = pkg.view.unwrap();
          let last_view = self.state.lock().last_view.clone();
          let viewc = self.state.lock().view_cp.clone();
          let cview = self.state.lock().view.clone();
          trace!("view pkg:{}, current view:{}， last view:{} ,view copy:{}",view, cview, last_view, viewc);
          if (((view == viewc-1)&&self.leadership.get_leader(viewc) == id)||
          ((view == viewc)&&self.leadership.get_leader(viewc) != id))&& viewc > cview {
          //if view == viewc-1 && viewc > cview {
            trace!("View jump successfully!");
            self.state.lock().success_jmp();
            //self.state.lock().multi_retain();
            self.state.lock().notify.notify_waiters();
          }
          let current_view = self.state.lock().view;
          if self.state.lock().abse_struct.get_r() < current_view {
            self.state.lock().abse_struct.update_round(current_view);
            self.state.lock().abse_struct.update();
            self.state.lock().abse_struct.set_info(Vec::new());
            trace!("{:?}: ABSE Struct", self.state.lock().abse_struct);
          }

          if !buffer.is_empty() {
              while let Some((&view, _)) = buffer.first_key_value() {
                  if view < current_view - 1 {
                      // Stale view
                      buffer.pop_first();
                      trace!("{}: stale view: {}", id, view);
                  } else if view > current_view {
                      break;
                  } else {
                      // It's time to process the pkg.
                      let pkgs: Vec<NetworkPackage> = buffer.pop_first().unwrap().1;
                      trace!(
                          "{}: process buffered (view: {}, current_view: {}) pkgs: {}",
                          id,
                          view,
                          current_view,
                          pkgs.len()
                      );
                      for pkg in pkgs.into_iter() {
                          self.process_message(
                              pkg,
                              id,
                              &mut voted_view,
                              &tx,
                              &finalized_block_tx,
                          )
                          .await;
                      }
                  }
              }
          }

          let current_view = self.state.lock().view;

          if view < current_view - 1 {
              // Stale view, drop it.
              continue;
          } else if view > current_view {
              // Received a message from future view, buffer it.
              trace!(
                  "{}: future (view: {}, current_view: {}) buffer pkg: {:?}",
                  id,
                  view,
                  current_view,
                  pkg
              );
              if let Some(v) = buffer.get_mut(&view) {
                  v.push(pkg);
              } else {
                  buffer.insert(view, vec![pkg]);
              }
          } else {
              // Deal with the messages larger than current view

              self.process_message(pkg, id, &mut voted_view, &tx, &finalized_block_tx)
                  .await;
          }
      }
  }

  async fn run_as_leader(self) {
      let id = self.state.lock().id;
      let batch_size = self.config.get_node_settings().batch_size;

      // println!("{}: leader start", id);

      loop {
          let tx = self.env.lock().network.get_sender();
          let view = self.state.lock().view;
          let last_view = self.state.lock().last_view;

          if self.leadership.get_leader(view) == id && view as i32 > last_view {
              tracing::trace!("{}: start as leader in view: {}", id, view);
              //self.state.lock().reset_array();
              let generic_qc = { self.state.lock().generic_qc.to_owned() };

              while self.collect_view.load(Ordering::SeqCst) + 1 < view {
                  tokio::task::yield_now().await;
              }

              // onPropose
              let generic_qc = self.state.lock().generic_qc.clone();
              let pkg = Self::new_key_block(self.env.to_owned(), view, generic_qc, id);
              tracing::trace!("{}: leader propose block in view: {}", id, view);
              tx.send(pkg).await.unwrap();
          }

          let notify = self.state.lock().notify.clone();
          // Get awoke if the view is changed.
          notify.notified().await;
          {
              let view = self.state.lock().view;
              trace!(
                  "{}: leader notified, view: {}, leader: {}",
                  id,
                  view,
                  self.leadership.get_leader(view)
              );
          }
      }
  }

  async fn run_as_pacemaker(self) {
      let timeout =
          tokio::time::Duration::from_millis(self.config.get_node_settings().timeout as u64);
      let tx = self.env.lock().network.get_sender();
      let id = self.config.get_id();

      let mut multiplexer = 1;

      loop {
          let past_view = self.state.lock().view;
          let next_awake = tokio::time::Instant::now() + timeout.mul_f64(multiplexer as f64);
          trace!("{}: pacemaker start", id);
          tokio::time::sleep_until(next_awake).await;
          trace!("{}: pacemaker awake", id);

          // If last vote is received later then 1s ago, then continue to sleep.
          let current_view = self.state.lock().view;
          if current_view != past_view {
              multiplexer = 1;
              continue;
          }

          warn!(
              "{} timeout!!! in view {}, leader: {}",
              id,
              current_view,
              self.leadership.get_leader(current_view)
          );

          // otherwise, try send a new-view message to nextleader
          let (next_leader, next_leader_view) = self.get_next_leader();
          trace!("{} send new_view to {}", id, next_leader);
          let pkg = self.new_new_view(next_leader_view, next_leader);
          tx.send(pkg).await.unwrap();
          self.state.lock().fail_jmp();
          self.state.lock().view = next_leader_view;
          self.state.lock().last_view = next_leader_view as i32 - 1;
          multiplexer += 1;
      }
  }

  fn new_new_view(&self, view: u64, next_leader: PublicKey) -> NetworkPackage {
      // latest Vote
      let digest = self.env.lock().block_tree.latest;
      let id = self.config.get_id();
      let signature = self.config.get_private_key().sign(&digest);
      let new_view =
          Message::NewView(self.state.lock().generic_qc.clone(), digest, id, signature);
      Self::package_message(self.state.lock().id, new_view, view, Some(next_leader))
  }

  // -> (leaderId, view)
  fn get_next_leader(&self) -> (PublicKey, u64) {
      let mut view = self.state.lock().view;
      let current_leader = self.leadership.get_leader(view);
      loop {
          view += 1;
          let next_leader = self.leadership.get_leader(view);
          if next_leader != current_leader {
              return (next_leader, view);
          }
      }
  }
}
