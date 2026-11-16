//! Account Management, pre-trade compliance, third-party, utility and SSO
//! endpoints under `/gw/api` (IBKR Web API reference, "Account Management",
//! "Pre-Trade Compliance", "Third Parties" and "Utilities").
//!
//! They need OAuth 2.0 ([`crate::OAuth2Config`]) with the scopes of the
//! endpoints in [`crate::OAuth2Config::scope`], for example
//! `"accounts.read accounts.write bank-instructions.write transfers.write"`.
//! GET requests carry the access token; POST and PATCH bodies are signed JWTs
//! (see [`WebClient::gw_request`]). Request bodies are the JSON objects of the
//! reference, passed as [`Value`].

use crate::client::{Result, WebClient, WebError, q, s, unix_now};
use crate::oauth2;
use reqwest::Method;
use serde_json::{Value, json};

impl WebClient {
    async fn gw_get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        self.gw_request(Method::GET, path, query, None).await
    }
    async fn gw_post(&self, path: &str, body: &Value) -> Result<Value> {
        self.gw_request(Method::POST, path, &[], Some(body)).await
    }
    async fn gw_patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.gw_request(Method::PATCH, path, &[], Some(body)).await
    }

    // ---- accounts (Account Management) -------------------------------------

    /// `GET /gw/api/v1/accounts`, optionally for one account or external id.
    pub async fn am_accounts(
        &self,
        account_id: Option<&str>,
        external_id: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("accountId", account_id.map(String::from)),
            ("externalId", external_id.map(String::from)),
        ]);
        self.gw_get("gw/api/v1/accounts", &query).await
    }
    /// `POST /gw/api/v1/accounts`: open an account (body: the application).
    pub async fn am_create_account(&self, application: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/accounts", application).await
    }
    /// `PATCH /gw/api/v1/accounts`: account maintenance requests.
    pub async fn am_update_accounts(&self, requests: &Value) -> Result<Value> {
        self.gw_patch("gw/api/v1/accounts", requests).await
    }
    /// `POST /gw/api/v1/accounts/documents`: submit agreements and forms.
    pub async fn am_submit_documents(&self, documents: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/accounts/documents", documents)
            .await
    }
    pub async fn am_account_details(&self, account_id: &str) -> Result<Value> {
        self.gw_get(&format!("gw/api/v1/accounts/{account_id}/details"), &[])
            .await
    }
    pub async fn am_account_kyc(&self, account_id: &str) -> Result<Value> {
        self.gw_get(&format!("gw/api/v1/accounts/{account_id}/kyc"), &[])
            .await
    }
    pub async fn am_account_login_messages(
        &self,
        account_id: &str,
        message_type: Option<&str>,
    ) -> Result<Value> {
        self.gw_get(
            &format!("gw/api/v1/accounts/{account_id}/login-messages"),
            &q(&[("type", message_type.map(String::from))]),
        )
        .await
    }
    /// `GET /gw/api/v1/accounts/login-messages?loginMessageRequest=<JSON>`.
    pub async fn am_login_messages(&self, request: &Value) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/accounts/login-messages",
            &q(&[("loginMessageRequest", s(request))]),
        )
        .await
    }
    pub async fn am_account_status(&self, account_id: &str) -> Result<Value> {
        self.gw_get(&format!("gw/api/v1/accounts/{account_id}/status"), &[])
            .await
    }
    /// `GET /gw/api/v1/accounts/status?accountStatusRequest=<JSON>`.
    pub async fn am_accounts_status(&self, request: &Value) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/accounts/status",
            &q(&[("accountStatusRequest", s(request))]),
        )
        .await
    }
    pub async fn am_update_account_status(&self, account_id: &str, body: &Value) -> Result<Value> {
        self.gw_patch(&format!("gw/api/v1/accounts/{account_id}/status"), body)
            .await
    }
    pub async fn am_account_tasks(
        &self,
        account_id: &str,
        task_type: Option<&str>,
    ) -> Result<Value> {
        self.gw_get(
            &format!("gw/api/v1/accounts/{account_id}/tasks"),
            &q(&[("type", task_type.map(String::from))]),
        )
        .await
    }
    pub async fn am_create_account_tasks(&self, account_id: &str, body: &Value) -> Result<Value> {
        self.gw_post(&format!("gw/api/v1/accounts/{account_id}/tasks"), body)
            .await
    }
    pub async fn am_update_account_tasks(&self, account_id: &str, body: &Value) -> Result<Value> {
        self.gw_patch(&format!("gw/api/v1/accounts/{account_id}/tasks"), body)
            .await
    }

    // ---- banking -------------------------------------------------------------

    /// `POST /gw/api/v1/bank-instructions` (`{"instructionType", "instruction"}`).
    pub async fn am_bank_instruction(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/bank-instructions", body).await
    }
    /// `POST /gw/api/v1/bank-instructions:bulk` (`{"instructionType", "instructions"}`).
    pub async fn am_bank_instructions_bulk(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/bank-instructions:bulk", body).await
    }
    pub async fn am_query_bank_instructions(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/bank-instructions/query", body)
            .await
    }
    pub async fn am_external_asset_transfer(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/external-asset-transfers", body)
            .await
    }
    /// `POST /gw/api/v2/external-asset-transfers`.
    pub async fn am_external_asset_transfer_v2(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v2/external-asset-transfers", body)
            .await
    }
    pub async fn am_external_asset_transfers_bulk(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/external-asset-transfers:bulk", body)
            .await
    }
    pub async fn am_external_cash_transfer(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/external-cash-transfers", body)
            .await
    }
    pub async fn am_external_cash_transfers_bulk(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/external-cash-transfers:bulk", body)
            .await
    }
    pub async fn am_query_external_cash_transfers(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/external-cash-transfers/query", body)
            .await
    }
    pub async fn am_internal_asset_transfer(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/internal-asset-transfers", body)
            .await
    }
    pub async fn am_internal_asset_transfers_bulk(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/internal-asset-transfers:bulk", body)
            .await
    }
    pub async fn am_internal_cash_transfer(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/internal-cash-transfers", body)
            .await
    }
    pub async fn am_internal_cash_transfers_bulk(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/internal-cash-transfers:bulk", body)
            .await
    }
    pub async fn am_cancel_instruction(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/instructions/cancel", body).await
    }
    pub async fn am_cancel_instructions_bulk(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/instructions/cancel:bulk", body)
            .await
    }
    pub async fn am_query_instructions(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/instructions/query", body).await
    }
    pub async fn am_instruction(&self, instruction_id: i64) -> Result<Value> {
        self.gw_get(&format!("gw/api/v1/instructions/{instruction_id}"), &[])
            .await
    }
    pub async fn am_client_instruction(&self, client_instruction_id: i64) -> Result<Value> {
        self.gw_get(
            &format!("gw/api/v1/client-instructions/{client_instruction_id}"),
            &[],
        )
        .await
    }
    pub async fn am_instruction_set(&self, instruction_set_id: i64) -> Result<Value> {
        self.gw_get(
            &format!("gw/api/v1/instruction-sets/{instruction_set_id}"),
            &[],
        )
        .await
    }

    // ---- reports ---------------------------------------------------------------

    /// `POST /gw/api/v1/statements` (`accountId`, `startDate`, `endDate`, ...).
    pub async fn am_statements(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/statements", body).await
    }
    pub async fn am_tax_documents(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/tax-documents", body).await
    }
    pub async fn am_trade_confirmations(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/trade-confirmations", body).await
    }
    pub async fn am_available_statements(&self, account_id: &str) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/statements/available",
            &q(&[("accountId", s(account_id))]),
        )
        .await
    }
    pub async fn am_available_tax_documents(&self, account_id: &str, year: i32) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/tax-documents/available",
            &q(&[("accountId", s(account_id)), ("year", s(year))]),
        )
        .await
    }
    pub async fn am_available_trade_confirmations(&self, account_id: &str) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/trade-confirmations/available",
            &q(&[("accountId", s(account_id))]),
        )
        .await
    }

    // ---- utilities ---------------------------------------------------------------

    /// `GET /gw/api/v1/enumerations/{type}` with optional filters
    /// (`currency`, `ibEntity`, `mdStatusNonPro`, `form-number`, `language`,
    /// `accountId`, `userName`, `type`, `citizenship`).
    pub async fn am_enumerations(
        &self,
        enumeration_type: &str,
        filters: &[(&str, &str)],
    ) -> Result<Value> {
        let query: Vec<(String, String)> = filters
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.gw_get(
            &format!("gw/api/v1/enumerations/{enumeration_type}"),
            &query,
        )
        .await
    }
    pub async fn am_complex_asset_transfer_enumerations(
        &self,
        instruction_type: &str,
    ) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/enumerations/complex-asset-transfer",
            &q(&[("instructionType", s(instruction_type))]),
        )
        .await
    }
    /// `GET /gw/api/v1/forms` with optional `formNo`, `getDocs`, `fromDate`,
    /// `toDate`, `language`, `projection`.
    pub async fn am_forms(&self, filters: &[(&str, &str)]) -> Result<Value> {
        let query: Vec<(String, String)> = filters
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.gw_get("gw/api/v1/forms", &query).await
    }
    /// `GET /gw/api/v1/forms/required-forms?requiredFormsRequest=<JSON>`.
    pub async fn am_required_forms(&self, request: &Value) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/forms/required-forms",
            &q(&[("requiredFormsRequest", s(request))]),
        )
        .await
    }
    pub async fn am_participating_banks(&self, bank_type: &str) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/participating-banks",
            &q(&[("type", s(bank_type))]),
        )
        .await
    }
    /// `GET /gw/api/v1/requests?requestDetails=<JSON>`.
    pub async fn am_requests(&self, request_details: &Value) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/requests",
            &q(&[("requestDetails", s(request_details))]),
        )
        .await
    }
    pub async fn am_request_status(&self, request_id: i64, request_type: &str) -> Result<Value> {
        self.gw_get(
            &format!("gw/api/v1/requests/{request_id}/status"),
            &q(&[("type", s(request_type))]),
        )
        .await
    }
    pub async fn am_update_request_status(&self, request_id: i64, body: &Value) -> Result<Value> {
        self.gw_patch(&format!("gw/api/v1/requests/{request_id}/status"), body)
            .await
    }
    pub async fn am_validate_username(&self, username: &str) -> Result<Value> {
        self.gw_get(&format!("gw/api/v1/validations/usernames/{username}"), &[])
            .await
    }

    // ---- third parties -------------------------------------------------------------

    /// `POST /gw/api/v1/balances/query`.
    pub async fn balances_query(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/balances/query", body).await
    }
    pub async fn tax_voucher_request(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/tax-vouchers", body).await
    }
    /// `GET /gw/api/v1/tax-vouchers/{requestId}/download`: the file bytes.
    pub async fn tax_voucher_download(&self, request_id: &str) -> Result<Vec<u8>> {
        let token = self.oauth2_access_token().await?;
        let cfg = self.oauth2_config()?;
        let url = crate::client::join(
            &cfg.auth_url,
            &format!("gw/api/v1/tax-vouchers/{request_id}/download"),
        );
        crate::client::fetch("GET", &url, self.http.get(&url).bearer_auth(token)).await
    }
    pub async fn tax_voucher_state(&self, request_id: &str) -> Result<Value> {
        self.gw_get(&format!("gw/api/v1/tax-vouchers/{request_id}/state"), &[])
            .await
    }
    pub async fn tax_voucher_dividends(
        &self,
        cust_acct_id: &str,
        year: i32,
        country_code: &str,
    ) -> Result<Value> {
        self.gw_get(
            "gw/api/v1/tax-vouchers/dividends",
            &q(&[
                ("custAcctId", s(cust_acct_id)),
                ("year", s(year)),
                ("countryCode", s(country_code)),
            ]),
        )
        .await
    }
    pub async fn tax_voucher_active_countries(&self) -> Result<Value> {
        self.gw_get("gw/api/v1/tax-vouchers/active-countries", &[])
            .await
    }
    pub async fn tax_voucher_years(&self) -> Result<Value> {
        self.gw_get("gw/api/v1/tax-vouchers/years", &[]).await
    }

    // ---- echo and SSO -----------------------------------------------------------------

    /// `GET /gw/api/v1/echo/https`: check the access token.
    pub async fn echo_https(&self) -> Result<Value> {
        self.gw_get("gw/api/v1/echo/https", &[]).await
    }
    /// `POST /gw/api/v1/echo/signed-jwt`: check a signed JWT body.
    pub async fn echo_signed_jwt(&self, body: &Value) -> Result<Value> {
        self.gw_post("gw/api/v1/echo/signed-jwt", body).await
    }
    /// `POST /gw/api/v1/sso-browser-sessions`: a browser SSO session for
    /// `credential` (username) from `ip`.
    pub async fn sso_browser_session(&self, credential: &str, ip: &str) -> Result<Value> {
        self.gw_post(
            "gw/api/v1/sso-browser-sessions",
            &json!({"credential": credential, "ip": ip}),
        )
        .await
    }

    // ---- pre-trade compliance ------------------------------------------------------------

    /// Send a pre-trade compliance request. IBKR wants an `Authorization:
    /// Bearer` JWT with an `accountId` claim (`master_account`) signed with
    /// the registered key, and for POST a second signed JWT, valid one minute,
    /// as the `text/plain` body.
    async fn restrictions_request(
        &self,
        method: Method,
        path: &str,
        master_account: &str,
        query: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let cfg = self.oauth2_config()?;
        let key = cfg.private_key()?;
        let now = unix_now();
        let header =
            oauth2::signed_body(cfg, &key, &json!({"accountId": master_account}), now, 60)?;
        let url = crate::client::join(&cfg.auth_url, path);
        let mut req = self
            .http
            .request(method.clone(), &url)
            .query(query)
            .bearer_auth(header);
        if let Some(b) = body {
            req = req
                .header("Content-Type", "text/plain")
                .body(oauth2::signed_body(cfg, &key, b, now, 60)?);
        }
        self.send(method.as_str(), &url, req).await
    }

    async fn restrictions_get(
        &self,
        path: &str,
        master_account: &str,
        master_user_name: &str,
        extra: (&str, &str),
        is_emp_track: bool,
    ) -> Result<Value> {
        let mut query = q(&[
            ("masterUserName", s(master_user_name)),
            ("isEmpTrack", is_emp_track.then(|| "T".to_string())),
        ]);
        if !extra.0.is_empty() {
            query.push((extra.0.into(), extra.1.into()));
        }
        self.restrictions_request(Method::GET, path, master_account, &query, None)
            .await
    }

    /// `POST /gw/api/v1/restrictions/verify`: check a CSV of restriction
    /// changes. `claims` are `userName`, `requestId`, `payload` (the CSV) and
    /// optionally `isEmpTrack`.
    pub async fn restrictions_verify(&self, master_account: &str, claims: &Value) -> Result<Value> {
        if !claims.is_object() {
            return Err(WebError::Invalid(format!(
                "restrictions_verify claims must be a JSON object with userName, requestId and payload, got {claims}"
            )));
        }
        self.restrictions_request(
            Method::POST,
            "gw/api/v1/restrictions/verify",
            master_account,
            &[],
            Some(claims),
        )
        .await
    }
    /// `POST /gw/api/v1/restrictions`: apply a verified CSV (same `requestId`).
    pub async fn restrictions_apply(&self, master_account: &str, claims: &Value) -> Result<Value> {
        if !claims.is_object() {
            return Err(WebError::Invalid(format!(
                "restrictions_apply claims must be a JSON object with userName, requestId and payload, got {claims}"
            )));
        }
        self.restrictions_request(
            Method::POST,
            "gw/api/v1/restrictions",
            master_account,
            &[],
            Some(claims),
        )
        .await
    }
    pub async fn restrictions_for_account(
        &self,
        master_account: &str,
        master_user_name: &str,
        account_id: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/account",
            master_account,
            master_user_name,
            ("accountId", account_id),
            is_emp_track,
        )
        .await
    }
    pub async fn restrictions_for_user(
        &self,
        master_account: &str,
        master_user_name: &str,
        user_name: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/user",
            master_account,
            master_user_name,
            ("userName", user_name),
            is_emp_track,
        )
        .await
    }
    pub async fn restriction_list(
        &self,
        master_account: &str,
        master_user_name: &str,
        list_id: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/list",
            master_account,
            master_user_name,
            ("listId", list_id),
            is_emp_track,
        )
        .await
    }
    pub async fn restriction_list_ids(
        &self,
        master_account: &str,
        master_user_name: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/lists/ids",
            master_account,
            master_user_name,
            ("", ""),
            is_emp_track,
        )
        .await
    }
    pub async fn restriction_ids(
        &self,
        master_account: &str,
        master_user_name: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/ids",
            master_account,
            master_user_name,
            ("", ""),
            is_emp_track,
        )
        .await
    }
    pub async fn restriction(
        &self,
        master_account: &str,
        master_user_name: &str,
        restriction_id: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/restriction",
            master_account,
            master_user_name,
            ("restrictionId", restriction_id),
            is_emp_track,
        )
        .await
    }
    pub async fn restriction_scope(
        &self,
        master_account: &str,
        master_user_name: &str,
        restriction_id: &str,
        is_emp_track: bool,
    ) -> Result<Value> {
        self.restrictions_get(
            "gw/api/v1/restrictions/restriction-scope",
            master_account,
            master_user_name,
            ("restrictionId", restriction_id),
            is_emp_track,
        )
        .await
    }
}
