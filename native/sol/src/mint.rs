pub async fn mint(
    candy_machine_id: Pubkey,
    candy_machine_state: Arc<CandyMachine>,
    collection_pda_info: Arc<Option<PdaInfo<CollectionPDA>>>,
    receiver: Pubkey,
) -> Result<Signature> {

    let rpc_url = "https://api.devnet.solana.com".to_string();
    let ws_url = rpc_url.replace("http", "ws");
    let cluster = Cluster::Custom(rpc_url, ws_url);

    // TODO: Get sugar config keypair from env
    // let key_bytes = Keypair::new().to_bytes();
    // let signer = Rc::new(Keypair::from_bytes(&key_bytes));
    let signer = Rc::new(Keypair::from_bytes(&[0u8; 32]).unwrap());

    let opts = CommitmentConfig::confirmed();
    let client = Client::new_with_options(cluster, signer, opts);


    let program = client.program(CANDY_MACHINE_ID);
    let payer = program.payer();
    let wallet = candy_machine_state.wallet;
    let authority = candy_machine_state.authority;

    let candy_machine_data = &candy_machine_state.data;

    if let Some(_gatekeeper) = &candy_machine_data.gatekeeper {
        return Err(anyhow!(
            "Command-line mint disabled (gatekeeper settings in use)"
        ));
    } else if candy_machine_state.items_redeemed >= candy_machine_data.items_available {
        return Err(anyhow!(CandyError::CandyMachineEmpty));
    }

    if candy_machine_state.authority != payer {
        // we are not authority, we need to follow the rules
        // 1. go_live_date
        // 2. whitelist mint settings
        // 3. end settings
        let mint_date = Utc::now().timestamp();
        let mut mint_enabled = if let Some(date) = candy_machine_data.go_live_date {
            // mint will be enabled only if the go live date is earlier
            // than the current date
            date < mint_date
        } else {
            // this is the case that go live date is null
            false
        };

        if let Some(wl_mint_settings) = &candy_machine_data.whitelist_mint_settings {
            if wl_mint_settings.presale {
                // we (temporarily) enable the mint - we will validate if the user
                // has the wl token when creating the transaction
                mint_enabled = true;
            } else if !mint_enabled {
                return Err(anyhow!(CandyError::CandyMachineNotLive));
            }
        }

        if !mint_enabled {
            // no whitelist mint settings (or no presale) and we are earlier than
            // go live date
            return Err(anyhow!(CandyError::CandyMachineNotLive));
        }

        if let Some(end_settings) = &candy_machine_data.end_settings {
            match end_settings.end_setting_type {
                EndSettingType::Date => {
                    if (end_settings.number as i64) < mint_date {
                        return Err(anyhow!(CandyError::CandyMachineNotLive));
                    }
                }
                EndSettingType::Amount => {
                    if candy_machine_state.items_redeemed >= end_settings.number {
                        return Err(anyhow!(
                            "Candy machine is not live (end settings amount reached)"
                        ));
                    }
                }
            }
        }
    }

    let nft_mint = Keypair::new();
    let metaplex_program_id = Pubkey::from_str(METAPLEX_PROGRAM_ID)?;

    // Allocate memory for the account
    let min_rent = program
        .rpc()
        .get_minimum_balance_for_rent_exemption(MINT_LAYOUT as usize)?;

    // Create mint account
    let create_mint_account_ix = system_instruction::create_account(
        &payer,
        &nft_mint.pubkey(),
        min_rent,
        MINT_LAYOUT,
        &TOKEN_PROGRAM_ID,
    );

    // Initialize mint ix
    let init_mint_ix = initialize_mint(
        &TOKEN_PROGRAM_ID,
        &nft_mint.pubkey(),
        &payer,
        Some(&payer),
        0,
    )?;

    // Derive associated token account
    let assoc = get_associated_token_address(&receiver, &nft_mint.pubkey());

    // Create associated account instruction
    let create_assoc_account_ix =
        create_associated_token_account(&payer, &receiver, &nft_mint.pubkey());

    // Mint to instruction
    let mint_to_ix = mint_to(
        &TOKEN_PROGRAM_ID,
        &nft_mint.pubkey(),
        &assoc,
        &payer,
        &[],
        1,
    )?;

    let mut additional_accounts: Vec<AccountMeta> = Vec::new();

    // Check whitelist mint settings
    if let Some(wl_mint_settings) = &candy_machine_data.whitelist_mint_settings {
        let whitelist_token_account = get_associated_token_address(&payer, &wl_mint_settings.mint);

        additional_accounts.push(AccountMeta {
            pubkey: whitelist_token_account,
            is_signer: false,
            is_writable: true,
        });

        if wl_mint_settings.mode == WhitelistMintMode::BurnEveryTime {
            let mut token_found = false;

            match program.rpc().get_account_data(&whitelist_token_account) {
                Ok(ata_data) => {
                    if !ata_data.is_empty() {
                        let account = Account::unpack_unchecked(&ata_data)?;

                        if account.amount > 0 {
                            additional_accounts.push(AccountMeta {
                                pubkey: wl_mint_settings.mint,
                                is_signer: false,
                                is_writable: true,
                            });

                            additional_accounts.push(AccountMeta {
                                pubkey: payer,
                                is_signer: true,
                                is_writable: false,
                            });

                            token_found = true;
                        }
                    }
                }
                Err(err) => {
                    error!("Invalid whitelist token account: {}", err);
                    return Err(anyhow!(
                        "Uninitialized whitelist token account: {whitelist_token_account}.
                         Check that you provided a valid SPL token mint for the whitelist."
                    ));
                }
            }

            if !token_found {
                return Err(anyhow!(CandyError::NoWhitelistToken));
            }
        }
    }

    if let Some(token_mint) = candy_machine_state.token_mint {
        let user_token_account_info = get_associated_token_address(&payer, &token_mint);

        additional_accounts.push(AccountMeta {
            pubkey: user_token_account_info,
            is_signer: false,
            is_writable: true,
        });

        additional_accounts.push(AccountMeta {
            pubkey: payer,
            is_signer: true,
            is_writable: false,
        });
    }

    let metadata_pda = find_metadata_pda(&nft_mint.pubkey());
    let master_edition_pda = find_master_edition_pda(&nft_mint.pubkey());
    let (candy_machine_creator_pda, creator_bump) =
        find_candy_machine_creator_pda(&candy_machine_id);

    let mut mint_ix = program
        .request()
        .accounts(nft_accounts::MintNFT {
            candy_machine: candy_machine_id,
            candy_machine_creator: candy_machine_creator_pda,
            payer,
            wallet,
            metadata: metadata_pda,
            mint: nft_mint.pubkey(),
            mint_authority: payer,
            update_authority: payer,
            master_edition: master_edition_pda,
            token_metadata_program: metaplex_program_id,
            token_program: TOKEN_PROGRAM_ID,
            system_program: system_program::id(),
            rent: sysvar::rent::ID,
            clock: sysvar::clock::ID,
            recent_blockhashes: sysvar::recent_blockhashes::ID,
            instruction_sysvar_account: sysvar::instructions::ID,
        })
        .args(nft_instruction::MintNft { creator_bump });

    // Add additional accounts directly to the mint instruction otherwise it won't work.
    if !additional_accounts.is_empty() {
        mint_ix = mint_ix.accounts(additional_accounts);
    }
    let mint_ix = mint_ix.instructions()?;

    let mut builder = program
        .request()
        .instruction(create_mint_account_ix)
        .instruction(init_mint_ix)
        .instruction(create_assoc_account_ix)
        .instruction(mint_to_ix)
        .instruction(mint_ix[0].clone())
        .signer(&nft_mint);

    if let Some((collection_pda_pubkey, collection_pda)) = collection_pda_info.as_ref() {
        let collection_authority_record =
            find_collection_authority_account(&collection_pda.mint, collection_pda_pubkey).0;

        let mut accounts = nft_accounts::SetCollectionDuringMint {
            candy_machine: candy_machine_id,
            metadata: metadata_pda,
            payer,
            collection_pda: *collection_pda_pubkey,
            token_metadata_program: mpl_token_metadata::ID,
            instructions: sysvar::instructions::ID,
            collection_mint: collection_pda.mint,
            collection_metadata: find_metadata_pda(&collection_pda.mint),
            collection_master_edition: find_master_edition_pda(&collection_pda.mint),
            authority,
            collection_authority_record,
        }
        .to_account_metas(None);

        // Make collection metadata account mutable for sized collections.
        accounts[7].is_writable = true;

        builder = builder
            .accounts(accounts)
            .args(nft_instruction::SetCollectionDuringMint {});
    }

    let sig = builder.send()?;

    if let Err(_) | Ok(Response { value: None, .. }) = program
        .rpc()
        .get_account_with_commitment(&metadata_pda, CommitmentConfig::processed())
    {
        let cluster_param = match get_cluster(program.rpc()).unwrap_or(Cluster::Mainnet) {
            Cluster::Devnet => "?devnet",
            _ => "",
        };
        return Err(anyhow!(
            "Minting most likely failed with a bot tax. Check the transaction link for more details: https://explorer.solana.com/tx/{}{}",
            sig.to_string(),
            cluster_param,
        ));
    }

    info!("Minted! TxId: {}", sig);

    Ok(sig)
}