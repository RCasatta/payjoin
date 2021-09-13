
#[cfg(all(feature = "sender", feature = "receiver"))]
mod integration {
    use bitcoind::bitcoincore_rpc::RpcApi;
    use bitcoind::bitcoincore_rpc;
    use bitcoin::Amount;
    use bip78::Uri;
    use std::str::FromStr;
    use bitcoin::util::psbt::PartiallySignedTransaction as Psbt;
    use log::{debug, log_enabled, Level};
    use std::collections::{HashMap, HashSet};
    use bip78::receiver::Headers;
    use bip78::receiver::state::{Validated, PsbtState, MaybeUnbroadcastable, TryNext};

    #[test]
    fn integration_test() {
        let _ = env_logger::try_init();
        let bitcoind_exe = std::env::var("BITCOIND_EXE")
            .ok()
            .or_else(|| bitcoind::downloaded_exe_path())
            .expect("version feature or env BITCOIND_EXE is required for tests");
        let mut conf = bitcoind::Conf::default();
        conf.view_stdout = log_enabled!(Level::Debug);
        let bitcoind = bitcoind::BitcoinD::with_conf(bitcoind_exe, &conf).unwrap();
        let receiver = bitcoind.create_wallet("receiver").unwrap();
        let receiver_address = receiver.get_new_address(None, None).unwrap();
        let sender = bitcoind.create_wallet("sender").unwrap();
        let sender_address = sender.get_new_address(None, None).unwrap();
        bitcoind.client.generate_to_address(1, &receiver_address).unwrap();
        bitcoind.client.generate_to_address(101, &sender_address).unwrap();

        assert_eq!(
            Amount::from_btc(50.0).unwrap(),
            receiver.get_balances().unwrap().mine.trusted,
            "receiver doesn't own bitcoin"
        );

        assert_eq!(
            Amount::from_btc(50.0).unwrap(),
            sender.get_balances().unwrap().mine.trusted,
            "sender doesn't own bitcoin"
        );

        // Receiver creates the payjoin URI
        let pj_receiver_address = receiver.get_new_address(None, None).unwrap();
        let amount = Amount::from_btc(1.0).unwrap();
        let pj_uri_string = format!("{}?amount={}&pj=https://example.com", pj_receiver_address.to_qr_uri(), amount.as_btc());
        let pj_uri = Uri::from_str(&pj_uri_string).unwrap();

        // Sender create a funded PSBT (not broadcasted) to address with amount given in the pj_uri
        let mut outputs = HashMap::with_capacity(1);
        outputs.insert(pj_uri.address().to_string(), pj_uri.amount().unwrap());
        debug!("outputs: {:?}", outputs);
        let options = bitcoincore_rpc::json::WalletCreateFundedPsbtOptions {
            lock_unspent: Some(true),
            fee_rate: Some(bip78::bitcoin::Amount::from_sat(2000)),
            ..Default::default()
        };
        let psbt = sender.wallet_create_funded_psbt(
            &[], // inputs
            &outputs,
            None, // locktime
            Some(options),
            None,
        ).expect("failed to create PSBT").psbt;
        let psbt = sender
            .wallet_process_psbt(&psbt, None, None, None)
            .unwrap()
            .psbt;
        let psbt = load_psbt_from_base64(psbt.as_bytes()).unwrap();
        debug!("Original psbt: {:#?}", psbt);
        let pj_params = bip78::sender::Params::with_fee_contribution(bip78::bitcoin::Amount::from_sat(10000), None);
        let (req, ctx) = pj_uri.create_request(psbt, pj_params).unwrap();
        let headers = HeaderMock::from_vec(&req.body);

        // Receiver receive payjoin proposal, IRL it will be an HTTP request (over ssl or onion)
        let validated = PsbtState::<Validated>::from_request(req.body.as_slice(), "", headers).unwrap();

        let mut maybe_broadcastable: PsbtState<MaybeUnbroadcastable> = validated.into();
        let tx = maybe_broadcastable.tx();
        let results = bitcoind.client.test_mempool_accept(&vec![&tx]).unwrap();
        if results.iter().any(|e| e.txid == tx.txid() && e.allowed) {
            maybe_broadcastable.verified_broadcastable();
        }

        let mut maybe_inputs_owned = maybe_broadcastable.try_next().unwrap();
        //TODO remove true || and properly verify
        if true || !maybe_inputs_owned.script_pubkeys().all(|s| {
            let address = bitcoin::Address::from_script(s, bitcoin::Network::Regtest).unwrap();  //TODO
            debug!("address: {}", address);
            let info = bitcoind.client.get_address_info(&address).unwrap();  //TODO
            !info.is_mine.unwrap()
        }) {
            maybe_inputs_owned.verified_inputs_not_owned();
        }

        let mut maybe_seen = maybe_inputs_owned.try_next().unwrap();
        let mut already_seen = HashSet::new();
        if maybe_seen.outpoints().all(|o| !already_seen.contains(&o) ) {
            maybe_seen.verified_prevouts_never_seen();
            already_seen.extend(maybe_seen.outpoints());
        }
        let proposal = maybe_seen.try_next().unwrap();


    }

    struct HeaderMock(HashMap<String, String>);

    impl Headers for HeaderMock {
        fn get_header(&self, key: &str) -> Option<&str> {
            self.0.get(key).map(|e| e.as_str())
        }
    }

    impl HeaderMock {
        fn from_vec(body: &[u8]) -> HeaderMock {
            let mut h = HashMap::new();
            h.insert("content-type".to_string(), "text/plain".to_string());
            h.insert("content-length".to_string(), body.len().to_string());
            HeaderMock(h)
        }
    }


    fn load_psbt_from_base64(mut input: impl std::io::Read) -> Result<Psbt, bip78::bitcoin::consensus::encode::Error> {
        use bip78::bitcoin::consensus::Decodable;

        let reader = base64::read::DecoderReader::new(&mut input, base64::Config::new(base64::CharacterSet::Standard, true));
        Psbt::consensus_decode(reader)
    }
}