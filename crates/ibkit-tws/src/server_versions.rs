//! Server version thresholds of the TWS API.
//!
//! Each constant is the first server version that supports a feature. The
//! names and values match `MIN_SERVER_VER_*` in IB's official client.

pub const REPLACE_FA_END: i32 = 157;
pub const DURATION: i32 = 158;
pub const MARKET_DATA_IN_SHARES: i32 = 159;
pub const POST_TO_ATS: i32 = 160;
pub const WSHE_CALENDAR: i32 = 161;
pub const AUTO_CANCEL_PARENT: i32 = 162;
pub const FRACTIONAL_SIZE_SUPPORT: i32 = 163;
pub const SIZE_RULES: i32 = 164;
pub const HISTORICAL_SCHEDULE: i32 = 165;
pub const ADVANCED_ORDER_REJECT: i32 = 166;
pub const USER_INFO: i32 = 167;
pub const CRYPTO_AGGREGATED_TRADES: i32 = 168;
pub const MANUAL_ORDER_TIME: i32 = 169;
pub const PEGBEST_PEGMID_OFFSETS: i32 = 170;
pub const WSH_EVENT_DATA_FILTERS: i32 = 171;
pub const IPO_PRICES: i32 = 172;
pub const WSH_EVENT_DATA_FILTERS_DATE: i32 = 173;
pub const INSTRUMENT_TIMEZONE: i32 = 174;
pub const HMDS_MARKET_DATA_IN_SHARES: i32 = 175;
pub const BOND_ISSUERID: i32 = 176;
pub const FA_PROFILE_DESUPPORT: i32 = 177;
pub const PENDING_PRICE_REVISION: i32 = 178;
pub const FUND_DATA_FIELDS: i32 = 179;
pub const MANUAL_ORDER_TIME_EXERCISE_OPTIONS: i32 = 180;
pub const OPEN_ORDER_AD_STRATEGY: i32 = 181;
pub const LAST_TRADE_DATE: i32 = 182;
pub const CUSTOMER_ACCOUNT: i32 = 183;
pub const PROFESSIONAL_CUSTOMER: i32 = 184;
pub const BOND_ACCRUED_INTEREST: i32 = 185;
pub const INELIGIBILITY_REASONS: i32 = 186;
pub const RFQ_FIELDS: i32 = 187;
pub const BOND_TRADING_HOURS: i32 = 188;
pub const INCLUDE_OVERNIGHT: i32 = 189;
pub const UNDO_RFQ_FIELDS: i32 = 190;
pub const PERM_ID_AS_LONG: i32 = 191;
pub const CME_TAGGING_FIELDS: i32 = 192;
pub const CME_TAGGING_FIELDS_IN_OPEN_ORDER: i32 = 193;
pub const ERROR_TIME: i32 = 194;
pub const FULL_ORDER_PREVIEW_FIELDS: i32 = 195;
pub const HISTORICAL_DATA_END: i32 = 196;
pub const CURRENT_TIME_IN_MILLIS: i32 = 197;
pub const SUBMITTER: i32 = 198;
pub const IMBALANCE_ONLY: i32 = 199;
pub const PARAMETRIZED_DAYS_OF_EXECUTIONS: i32 = 200;
pub const PROTOBUF: i32 = 201;
pub const ZERO_STRIKE: i32 = 202;
pub const PROTOBUF_PLACE_ORDER: i32 = 203;
pub const PROTOBUF_COMPLETED_ORDER: i32 = 204;
pub const PROTOBUF_CONTRACT_DATA: i32 = 205;
pub const PROTOBUF_MARKET_DATA: i32 = 206;
pub const PROTOBUF_ACCOUNTS_POSITIONS: i32 = 207;
pub const PROTOBUF_HISTORICAL_DATA: i32 = 208;
pub const PROTOBUF_NEWS_DATA: i32 = 209;
pub const PROTOBUF_SCAN_DATA: i32 = 210;
pub const PROTOBUF_REST_MESSAGES_1: i32 = 211;
pub const PROTOBUF_REST_MESSAGES_2: i32 = 212;
pub const PROTOBUF_REST_MESSAGES_3: i32 = 213;
pub const ADD_Z_SUFFIX_TO_UTC_DATE_TIME: i32 = 214;
pub const CANCEL_CONTRACT_DATA: i32 = 215;
pub const ADDITIONAL_ORDER_PARAMS_1: i32 = 216;
pub const ADDITIONAL_ORDER_PARAMS_2: i32 = 217;
pub const ATTACHED_ORDERS: i32 = 218;
pub const CONFIG: i32 = 219;
pub const MARKET_DATA_VOLUMES_IN_SHARES: i32 = 220;
pub const UPDATE_CONFIG: i32 = 221;
pub const FRACTIONAL_LAST_SIZE: i32 = 222;
pub const HEDGE_MAX_SIZE: i32 = 223;
pub const USE_PRECISION_FROM_SEC_DEF: i32 = 224;
pub const ODD_LOT_BID_ASK_QUOTES: i32 = 225;
pub const COND_ORDER_WITH_OVERNIGHT_PARAM: i32 = 226;

/// Lowest server version this crate accepts (same as ib_async).
pub const MIN_CLIENT_VER: i32 = 157;
/// Highest server version this crate announces. From 201 the protobuf
/// protocol is used where IB's client uses it.
pub const MAX_CLIENT_VER: i32 = COND_ORDER_WITH_OVERNIGHT_PARAM;
