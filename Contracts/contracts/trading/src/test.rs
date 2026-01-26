
// We need to import the social rewards contract for testing
// In a workspace, we can register the contract by its WASM, but here we can just register the struct if it's available.
// Since it's a separate crate, we can't easily import the struct without adding it to dependencies.
// However, for unit testing within `trading` crate, we can Mock the reward contract or use `register_contract_wasm` if we had the wasm.
// 
// Alternatively, we can define a mock contract HERE in the test module.

#[contract]
pub struct MockRewardContract;

#[contractimpl]
impl MockRewardContract {
    pub fn add_reward(env: Env, user: Address, amount: i128) {
        if amount <= 0 {
            panic!("Invalid reward amount");
        }
        // Success
    }
}

#[test]
fn test_pause_functionality() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, TradingContract);
    let client = TradingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    
    client.initialize(&admin);

    // Initial state: Not paused
    assert_eq!(client.is_paused(), false);

    // Pause the contract
    client.set_pause(&true);
    assert_eq!(client.is_paused(), true);

    // Try trade (should fail)
    let trader = Address::generate(&env);
    let fee_token = Address::generate(&env); // Just random address for now
    let recipient = Address::generate(&env);
    
    let res = client.try_trade(&trader, &fee_token, &100, &recipient);
    assert!(res.is_err());

    // Unpause
    client.set_pause(&false);
    assert_eq!(client.is_paused(), false);
    
    // Trade should fail with something else (like fee error) but NOT pause error
    // If we mock tokens it would succeed. 
    // Let's just check it is not the PAUSED error if we could match. 
    // Or we can just set up enough to pass past the pause check.
    // The pause check is the FIRST thing.
    // If we get past it, we hit FeeManager which might fail.
    
    // Let's set up tokens so trade succeeds
    let issuer = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract(issuer);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_contract_id);
    token_admin_client.mint(&trader, &1000);

    let res_ok = client.try_trade(&trader, &token_contract_id, &100, &recipient);
    assert!(res_ok.is_ok());
}

#[test]
fn test_trade_and_reward_success() {
    let env = Env::default();
    env.mock_all_auths();

    let trading_id = env.register_contract(None, TradingContract);
    let trading_client = TradingContractClient::new(&env, &trading_id);

    let reward_id = env.register_contract(None, MockRewardContract);
    
    // Setup Tokens
    let issuer = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract(issuer);
    let token_client = token::Client::new(&env, &token_id);
    let token_admin = token::StellarAssetClient::new(&env, &token_id);

    let trader = Address::generate(&env);
    let recipient = Address::generate(&env);
    let fee = 100;

    token_admin.mint(&trader, &1000);

    // Run trade_and_reward
    let res = trading_client.trade_and_reward(
        &trader, 
        &token_id, 
        &fee, 
        &recipient, 
        &reward_id, 
        &50 // valid reward
    );

    assert!(res.is_ok());

    // Verify fee was paid
    assert_eq!(token_client.balance(&trader), 900);
    assert_eq!(token_client.balance(&recipient), 100);
}

#[test]
fn test_trade_and_reward_atomic_rollback() {
    let env = Env::default();
    env.mock_all_auths();

    let trading_id = env.register_contract(None, TradingContract);
    let trading_client = TradingContractClient::new(&env, &trading_id);

    let reward_id = env.register_contract(None, MockRewardContract);
    
    // Setup Tokens
    let issuer = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract(issuer);
    let token_client = token::Client::new(&env, &token_id);
    let token_admin = token::StellarAssetClient::new(&env, &token_id);

    let trader = Address::generate(&env);
    let recipient = Address::generate(&env);
    let fee = 100;

    token_admin.mint(&trader, &1000);

    // Run trade_and_reward with INVALID reward amount (0)
    // This will cause MockRewardContract to panic.
    // TradingContract::trade_and_reward uses safe_invoke, which catches the panic
    // and returns SafeCallErrors::CALL_FAILED (mapped to u32).
    // The TradingContract then returns Err.
    // The ENV should revert all changes, including the fee payment.
    
    // Note: We use try_trade_and_reward to inspect the error result
    let res = trading_client.try_trade_and_reward(
        &trader, 
        &token_id, 
        &fee, 
        &recipient, 
        &reward_id, 
        &0 // Invalid reward amount -> panic
    );

    // The result should be an Err
    assert!(res.is_err());
    
    // Check error code
    match res {
        Err(Ok(code)) => {
             // We expect CALL_FAILED (2001)
             assert_eq!(code, SafeCallErrors::CALL_FAILED);
        },
        _ => panic!("Expected contract error code"),
    }

    // CRITICAL: Verify ATOMICITY
    // The fee transfer (which happened before the cross-call) must be rolled back.
    // Trader balance should still be 1000.
    assert_eq!(token_client.balance(&trader), 1000);
    assert_eq!(token_client.balance(&recipient), 0);
}
