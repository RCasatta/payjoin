
#[cfg(all(feature = "sender", feature = "receiver"))]
mod test {
    use bitcoind::bitcoincore_rpc::{Client, RpcApi, self};
    use bitcoin::{Amount, OutPoint, Address, Network};
    use bip78::Uri;
    use std::str::FromStr;
    use bitcoin::util::psbt::PartiallySignedTransaction as Psbt;
    use log::{debug, log_enabled, Level};
    use std::collections::{HashMap, HashSet};
    use bip78::bitcoin::{Transaction, Script};
    use assert_matches::assert_matches;

    #[test]
    fn integration_test() {

        let _ = env_logger::try_init();
        let bitcoind_exe = std::env::var("BITCOIND_EXE")
            .ok()
            .or_else(|| bitcoind::downloaded_exe_path())
            .expect("version feature or env BITCOIND_EXE is required for tests");
        let conf = bitcoind::Conf {
            view_stdout: log_enabled!(Level::Debug),
            ..Default::default()
        };
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
        let pj_uri_string = bip78::receiver::create_uri(&pj_receiver_address, &amount, "https://example.com");
        let pj_uri = Uri::from_str(&pj_uri_string).unwrap();

        // Sender create a funded PSBT (not broadcasted) to address with amount given in the pj_uri
        let mut outputs = HashMap::with_capacity(1);
        outputs.insert(pj_uri.address().to_string(), pj_uri.amount());
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
        let unchecked_proposal = bip78::receiver::UncheckedProposal::from_request(req.body.as_slice(), "",headers).unwrap();
        let mut bitcoind_checker = BitcoindChecker::new(&receiver);

        let proposal = unchecked_proposal.check(&mut bitcoind_checker).unwrap();

        // TODO add receiver input and change outputs

    }

    struct BitcoindChecker<'a> {
        client: &'a Client,
        /// This is a mockup implementation, OutPoint seen should be persisted across session or attacks are possible
        seen: HashSet<OutPoint>,
        network: Network,
    }

    impl<'a> BitcoindChecker<'a> {
        pub fn new(client: &'a Client) -> Self {

            BitcoindChecker {
                client,
                seen: HashSet::new(),
                network: Network::Regtest, //TODO
            }
        }
    }

    impl<'a> Checks for BitcoindChecker<'a> {
        fn unbroacastable(&self, tx: &Transaction) -> bool {
            let results = self.client.test_mempool_accept(&vec![tx]).unwrap(); //TODO
            !results.iter().any(|e| e.txid == tx.txid() && e.allowed)
        }

        fn already_seen(&mut self, out_point: &OutPoint) -> bool {
            !self.seen.insert(out_point.clone())
        }

        fn owned(&self, script_pubkey: &Script) -> bool {
            let address = Address::from_script(script_pubkey, self.network).unwrap();  //TODO
            debug!("address: {}", address);
            let info = self.client.get_address_info(&address).unwrap();  //TODO
            info.is_mine.unwrap()
        }
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
