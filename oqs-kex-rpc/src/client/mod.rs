// Copyright 2017 Amagicom AB.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use oqs;
use oqs::kex::{AliceMsg, BobMsg, OqsKex, OqsKexAlg, OqsKexAlice, SharedKey};
use oqs::rand::{OqsRand, OqsRandAlg};

use jsonrpc_client_http::HttpHandle;

mod rpc;

error_chain! {
    errors {
        /// There was an error in the network communication or on the key exchange server.
        RpcError { description("RPC client returned an error") }
        /// The server responded, but the returned messages don't match our request.
        InvalidResponse { description("RPC response is syntactically valid but unexpected") }
        /// There was an error in the cryptographic operations in `oqs`.
        OqsError { description("OQS returned an error") }
    }
}

/// The key exchange client.
pub struct OqsKexClient {
    rpc_client: rpc::OqsKexRpcClient<HttpHandle>,
    rand: OqsRandAlg,
}

impl OqsKexClient {
    /// Connects to the given address and returns a client instance.
    pub fn new(server_uri: &str) -> Result<Self> {
        let rpc_client =
            rpc::OqsKexRpcClient::connect(server_uri).chain_err(|| ErrorKind::RpcError)?;

        let client = OqsKexClient {
            rpc_client,
            rand: OqsRandAlg::default(),
        };

        Ok(client)
    }

    /// Configure which PRNG algorithm this client should use to source its entropy.
    pub fn set_rand(&mut self, rand: OqsRandAlg) {
        self.rand = rand;
    }

    /// Performs a full key exchange with all the algorithms in `algs` at the same time.
    ///
    /// This will compute Alice's message for each given algorithm, and send them in one RPC
    /// call to the server. The server will then compute the corresponding shared keys and Bob's
    /// messages. Then the server return Bob's messages and this client finally computes
    /// the shared keys and returns them.
    ///
    /// The returned vector has the same length as `algs` and the [`SharedKey`] at position `n`
    /// corresponds to the [`OqsKexAlg`] at position `n` in `algs`.
    ///
    /// [`SharedKey`]: struct.SharedKey.html
    /// [`OqsKexAlg`]: struct.OqsKexAlg.html
    pub fn kex(&mut self, algs: &[OqsKexAlg]) -> Result<Vec<SharedKey>> {
        let rand = OqsRand::new(self.rand).chain_err(|| ErrorKind::OqsError)?;
        let kexs = Self::init_kex(&rand, algs)?;
        let alice_kexs = Self::alice_0(&kexs)?;
        let bob_msgs = self.perform_rpc(&alice_kexs)?;
        ensure!(
            alice_kexs.len() == bob_msgs.len(),
            ErrorKind::InvalidResponse
        );
        for (alice_kex, bob_msg) in alice_kexs.iter().zip(bob_msgs.iter()) {
            ensure!(
                alice_kex.algorithm() == bob_msg.algorithm(),
                ErrorKind::InvalidResponse
            )
        }
        Self::alice_1(alice_kexs, &bob_msgs)
    }

    fn init_kex<'r>(rand: &'r OqsRand, algs: &[OqsKexAlg]) -> Result<Vec<OqsKex<'r>>> {
        algs.iter()
            .map(|alg| OqsKex::new(&rand, *alg))
            .collect::<oqs::kex::Result<_>>()
            .chain_err(|| ErrorKind::OqsError)
    }

    fn alice_0<'a, 'r>(kexs: &'a [OqsKex<'r>]) -> Result<Vec<OqsKexAlice<'a, 'r>>> {
        kexs.iter()
            .map(|kex| kex.alice_0())
            .collect::<oqs::kex::Result<_>>()
            .chain_err(|| ErrorKind::OqsError)
    }

    fn alice_1(alice_kexs: Vec<OqsKexAlice>, bob_msgs: &[BobMsg]) -> Result<Vec<SharedKey>> {
        alice_kexs
            .into_iter()
            .zip(bob_msgs)
            .map(|(alice_kex, bob_msg)| alice_kex.alice_1(&bob_msg))
            .collect::<oqs::kex::Result<_>>()
            .chain_err(|| ErrorKind::OqsError)
    }

    fn perform_rpc(&mut self, alice_kexs: &[OqsKexAlice]) -> Result<Vec<BobMsg>> {
        let alice_msgs: Vec<&AliceMsg> =
            alice_kexs.iter().map(OqsKexAlice::get_alice_msg).collect();
        self.rpc_client
            .kex(&alice_msgs)
            .call()
            .chain_err(|| ErrorKind::RpcError)
    }
}
