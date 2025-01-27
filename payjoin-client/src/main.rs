use std::collections::HashMap;
use bip78::bitcoin::util::psbt::PartiallySignedTransaction as Psbt;
use bitcoincore_rpc::RpcApi;

fn main() {
    let mut args = std::env::args_os();
    let _program_name = args
        .next()
        .expect("not even program name given");
    let port = args
        .next()
        .expect("Missing arguments: port cookie_file bip21")
        .into_string()
        .expect("port is not UTF-8")
        .parse::<u16>()
        .expect("port must be a number");

    let cookie_file = args
        .next()
        .expect("Missing arguments: cookie_file bip21");

    let bip21 = args
        .next()
        .expect("Missing arguments: bip21")
        .into_string()
        .expect("bip21 is not UTF-8");


    let link = bip21.parse::<bip78::Uri>().unwrap();
    let mut outputs = HashMap::with_capacity(1);
    outputs.insert(link.address().to_string(), link.amount());

    let client = bitcoincore_rpc::Client::new(format!("http://127.0.0.1:{}", port), bitcoincore_rpc::Auth::CookieFile(cookie_file.into())).unwrap();
    let options = bitcoincore_rpc::json::WalletCreateFundedPsbtOptions {
        lock_unspent: Some(true),
        fee_rate: Some(bip78::bitcoin::Amount::from_sat(2000)),
        ..Default::default()
    };
    let psbt = client.wallet_create_funded_psbt(
        &[], // inputs
        &outputs,
        None, // locktime
        Some(options),
        None,
    ).expect("failed to create PSBT").psbt;
    let psbt = client
        .wallet_process_psbt(&psbt, None, None, None)
        .unwrap()
        .psbt;
    let psbt = load_psbt_from_base64(psbt.as_bytes()).unwrap();
    println!("Original psbt: {:#?}", psbt);
    let pj_params = bip78::sender::Params::with_fee_contribution(bip78::bitcoin::Amount::from_sat(10000), None);
    let (req, ctx) = link.create_request(psbt, pj_params).unwrap();
    let response = reqwest::blocking::Client::new()
        .post(&req.url)
        .body(req.body)
        .header("Content-Type", "text/plain")
        .send()
        .expect("failed to communicate");
        //.error_for_status()
        //.unwrap();
    let psbt = ctx.process_response(response).unwrap();
    println!("Proposed psbt: {:#?}", psbt);
    let psbt = client
        .wallet_process_psbt(&serialize_psbt(&psbt), None, None, None)
        .unwrap()
        .psbt;
    let tx = client
        .finalize_psbt(&psbt, Some(true))
        .unwrap()
        .hex
        .expect("incomplete psbt");
    client.send_raw_transaction(&tx).unwrap();
}

fn load_psbt_from_base64(mut input: impl std::io::Read) -> Result<Psbt, bip78::bitcoin::consensus::encode::Error> {
    use bip78::bitcoin::consensus::Decodable;    
 
    let reader = base64::read::DecoderReader::new(&mut input, base64::Config::new(base64::CharacterSet::Standard, true));
    Psbt::consensus_decode(reader)    
}

fn serialize_psbt(psbt: &Psbt) -> String {
    use bip78::bitcoin::consensus::Encodable;
                                    
    let mut encoder = base64::write::EncoderWriter::new(Vec::new(), base64::STANDARD);
    psbt.consensus_encode(&mut encoder)
        .expect("Vec doesn't return errors in its write implementation");
    String::from_utf8(encoder.finish()
        .expect("Vec doesn't return errors in its write implementation")).unwrap()
}
