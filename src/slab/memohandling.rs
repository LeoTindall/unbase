use super::*;

impl SlabShared {
    pub fn check_memo_waiters ( &mut self, memo: &Memo) {
        match self.memo_wait_channels.entry(memo.id) {
            Entry::Occupied(o) => {
                for channel in o.get() {
                    // we don't care if it worked or not.
                    // if the channel is closed, we're scrubbing it anyway
                    channel.send(memo.clone()).ok();
                }
                o.remove();
            },
            Entry::Vacant(_) => {}
        };
    }

    pub fn handle_memo_from_other_slab( &mut self, memo: &Memo, memoref: &MemoRef, origin_slabref: &SlabRef, origin_peering_status: &MemoPeeringStatus, my_slab: &Slab, my_ref: &SlabRef  ){

        match memo.inner.body {
            // This Memo is a peering status update for another memo
            MemoBody::SlabPresence{ p: ref presence, r: ref opt_root_index_seed } => {

                let should_process;
                match opt_root_index_seed {
                    &Some(ref root_index_seed) => {
                        // HACK - this should be done inside the deserialize
                        for memoref in root_index_seed.iter() {
                            memoref.update_peer(origin_slabref, MemoPeeringStatus::Resident);
                        }

                        should_process = self.net.apply_root_index_seed( &presence, root_index_seed );
                    }
                    &None => {
                        should_process = true;
                    }
                }

                if should_process {
                    if let Ok(mentioned_slabref) = self.slabref_from_presence( presence ) {
                        // TODO: should we be telling the origin slabref, or the presence slabref that we're here?
                        //       these will usually be the same, but not always

                        let my_presence_memoref = self.new_memo_basic(
                            None,
                            memoref.to_head(),
                            MemoBody::SlabPresence{
                                p: self.presence_for_origin( origin_slabref ),
                                r: self.get_root_index_seed()
                            }
                        );

                        origin_slabref.send( &self.my_ref, my_presence_memoref );

                    }
                }
            }
            MemoBody::Peering(memo_id, subject_id, ref peerlist ) => {
                let (peered_memoref,had) = self.memoref( memo_id, subject_id, peerlist );

                // Don't peer with yourself
                for peer in peerlist.0.iter().filter(|p| p.slabref.0.slab_id != self.id ) {
                    peered_memoref.update_peer( &peer.slabref, peer.status.clone());
                }
            },
            MemoBody::MemoRequest(ref desired_memo_ids, ref requesting_slabref ) => {

                if requesting_slabref.0.slab_id != self.id {
                    for desired_memo_id in desired_memo_ids {
                        if let Some(desired_memoref) = self.memorefs_by_id.get(&desired_memo_id) {

                            if desired_memoref.is_resident() {
                                requesting_slabref.send(&my_ref, desired_memoref)
                            } else {
                                // Somebody asked me for a memo I don't have
                                // It would be neighborly to tell them I don't have it
                                let peering_memo = Memo::new(
                                    my_slab.gen_memo_id(),
                                    None,
                                    MemoRefHead::from_memoref(memoref.clone()),
                                    MemoBody::Peering( memo.id, memo.subject_id, my_ref.get_presence(), MemoPeeringStatus::Participating),
                                    &my_slab
                                );
                                requesting_slabref.send_memo(&my_ref, peering_memo)
                            }
                        }else{
                            let peering_memo = Memo::new(
                                my_slab.gen_memo_id(),
                                None,
                                MemoRefHead::from_memoref(memoref.clone()),
                                MemoBody::Peering(
                                    memo.id,
                                    memo.subject_id,
                                    my_ref.get_presence(),
                                    MemoPeeringStatus::NonParticipating
                                )
                            );
                            requesting_slabref.send(&my_ref, peering_memo)
                        }
                    }
                }
            }
            _ => {}
        }
    }
    pub fn do_peering_for_memo(&self, memo: &Memo, memoref: &MemoRef, origin_slabref: &SlabRef, my_slab: &Slab, my_slabref: &SlabRef) {
        // Peering memos don't get peering memos, but Edit memos do
        // Abstracting this, because there might be more types that don't do peering
        if memo.does_peering() {
            // That we received the memo means that the sender didn't think we had it
            // Whether or not we had it already, lets tell them we have it now.
            // It's useful for them to know we have it, and it'll help them STFU

            // TODO: determine if peering memo should:
            //    A. use parents at all
            //    B. and if so, what should be should we be using them for?
            //    C. Should we be sing that to determine the peered memo instead of the payload?
            //println!("MEOW {}, {:?}", my_ref );

            let peering_memoref = self.new_memo(
                None,
                memoref.to_head(),
                MemoBody::Peering(
                    memo.id,
                    memo.subject_id,
                    memoref.get_peerlist_for_peer(&self.my_ref, origin_slabref)
                )
            );
            origin_slabref.send( &self.my_ref, peering_memoref );
        }

    }

    pub fn emit_memos(&self, memorefs: &Vec<MemoRef>) {
        // Emit memos for durability and notification purposes
        // At present, some memos like peering and slab presence are emitted manually.
        // TODO: This will almost certainly have to change once gossip/plumtree functionality is added

        // TODO: test each memo for durability_score and emit accordingly
        for memoref in memorefs.iter() {
            if let Some(memo) = memoref.get_memo_if_resident() {
                let needs_peers = self.check_peering_target(&memo);

                for peer_ref in self.peer_refs.iter().filter(|x| !memoref.is_peered_with_slabref(x) ).take( needs_peers as usize ) {
                    println!("# Slab({}).emit_memos - EMIT Memo {} to Slab {}", my_ref.0.slab_id, memo.id, peer_ref.0.slab_id );
                    peer_ref.send( &self.my_ref, memoref );
                }
            }
        }

    }
}
