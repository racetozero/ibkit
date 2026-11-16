//! Live market data container (ib_async `Ticker`) and the tick type mapping.

use crate::objects::*;
use ibkit_tws::decode::{EfpData, OptionComputation};
use ibkit_tws::types::Contract;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Values used for "no price" and "no size" (ib_async `IBDefaults`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    pub empty_price: f64,
    pub empty_size: f64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            empty_price: -1.0,
            empty_size: 0.0,
        }
    }
}

macro_rules! ticker_struct {
    ($($field:ident),* $(,)?) => {
        /// Latest market data of one contract. Float fields start as NaN.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Ticker {
            pub contract: Contract,
            pub time: Option<Timestamp>,
            pub market_data_type: i32,
            pub min_tick: f64,
            pub bid_exchange: String,
            pub ask_exchange: String,
            pub last_exchange: String,
            pub last_timestamp: Option<Timestamp>,
            pub rt_time: Option<Timestamp>,
            pub last_reg_time: String,
            pub option_bid_exch: String,
            pub option_ask_exch: String,
            pub delayed_last_timestamp: Option<Timestamp>,
            pub reuters_mutual_funds: String,
            pub social_market_analytics: String,
            pub odd_lot_bid_exchange: String,
            pub odd_lot_ask_exchange: String,
            pub bbo_exchange: String,
            pub snapshot_permissions: i32,
            pub dividends: Option<Dividends>,
            pub fundamental_ratios: Option<HashMap<String, f64>>,
            pub ticks: Vec<TickData>,
            pub tick_by_ticks: Vec<TickByTick>,
            pub dom_bids: Vec<DOMLevel>,
            pub dom_asks: Vec<DOMLevel>,
            #[serde(skip)]
            pub dom_bids_dict: BTreeMap<i32, DOMLevel>,
            #[serde(skip)]
            pub dom_asks_dict: BTreeMap<i32, DOMLevel>,
            pub dom_ticks: Vec<MktDepthData>,
            pub bid_greeks: Option<OptionComputation>,
            pub ask_greeks: Option<OptionComputation>,
            pub last_greeks: Option<OptionComputation>,
            pub model_greeks: Option<OptionComputation>,
            pub cust_greeks: Option<OptionComputation>,
            pub bid_efp: Option<EfpData>,
            pub ask_efp: Option<EfpData>,
            pub last_efp: Option<EfpData>,
            pub open_efp: Option<EfpData>,
            pub high_efp: Option<EfpData>,
            pub low_efp: Option<EfpData>,
            pub close_efp: Option<EfpData>,
            $(pub $field: f64,)*
        }

        impl Ticker {
            pub fn new(contract: Contract) -> Self {
                Self {
                    contract,
                    time: None,
                    market_data_type: 1,
                    min_tick: f64::NAN,
                    bid_exchange: String::new(),
                    ask_exchange: String::new(),
                    last_exchange: String::new(),
                    last_timestamp: None,
                    rt_time: None,
                    last_reg_time: String::new(),
                    option_bid_exch: String::new(),
                    option_ask_exch: String::new(),
                    delayed_last_timestamp: None,
                    reuters_mutual_funds: String::new(),
                    social_market_analytics: String::new(),
                    odd_lot_bid_exchange: String::new(),
                    odd_lot_ask_exchange: String::new(),
                    bbo_exchange: String::new(),
                    snapshot_permissions: 0,
                    dividends: None,
                    fundamental_ratios: None,
                    ticks: Vec::new(),
                    tick_by_ticks: Vec::new(),
                    dom_bids: Vec::new(),
                    dom_asks: Vec::new(),
                    dom_bids_dict: BTreeMap::new(),
                    dom_asks_dict: BTreeMap::new(),
                    dom_ticks: Vec::new(),
                    bid_greeks: None,
                    ask_greeks: None,
                    last_greeks: None,
                    model_greeks: None,
                    cust_greeks: None,
                    bid_efp: None,
                    ask_efp: None,
                    last_efp: None,
                    open_efp: None,
                    high_efp: None,
                    low_efp: None,
                    close_efp: None,
                    $($field: f64::NAN,)*
                }
            }
        }
    };
}

ticker_struct!(
    bid,
    bid_size,
    ask,
    ask_size,
    last,
    last_size,
    prev_bid,
    prev_bid_size,
    prev_ask,
    prev_ask_size,
    prev_last,
    prev_last_size,
    volume,
    open,
    high,
    low,
    close,
    vwap,
    low13week,
    high13week,
    low26week,
    high26week,
    low52week,
    high52week,
    bid_yield,
    ask_yield,
    last_yield,
    mark_price,
    halted,
    rt_hist_volatility,
    rt_volume,
    rt_trade_volume,
    av_volume,
    trade_count,
    trade_rate,
    volume_rate,
    volume_rate3_min,
    volume_rate5_min,
    volume_rate10_min,
    shortable,
    shortable_shares,
    index_future_premium,
    futures_open_interest,
    put_open_interest,
    call_open_interest,
    put_volume,
    call_volume,
    av_option_volume,
    hist_volatility,
    implied_volatility,
    open_interest,
    last_rth_trade,
    bond_factor_multiplier,
    creditman_mark_price,
    creditman_slow_mark_price,
    delayed_halted,
    etf_nav_close,
    etf_nav_prior_close,
    etf_nav_bid,
    etf_nav_ask,
    etf_nav_last,
    etf_frozen_nav_last,
    etf_nav_high,
    etf_nav_low,
    estimated_ipo_midpoint,
    final_ipo_last,
    auction_volume,
    auction_price,
    auction_imbalance,
    regulatory_imbalance,
    odd_lot_bid,
    odd_lot_ask,
    odd_lot_bid_size,
    odd_lot_ask_size,
);

fn price_field(t: &mut Ticker, tick_type: i32) -> Option<&mut f64> {
    Some(match tick_type {
        6 | 72 => &mut t.high,
        7 | 73 => &mut t.low,
        9 | 75 => &mut t.close,
        14 | 76 => &mut t.open,
        15 => &mut t.low13week,
        16 => &mut t.high13week,
        17 => &mut t.low26week,
        18 => &mut t.high26week,
        19 => &mut t.low52week,
        20 => &mut t.high52week,
        35 => &mut t.auction_price,
        37 => &mut t.mark_price,
        50 | 103 => &mut t.bid_yield,
        51 | 104 => &mut t.ask_yield,
        52 => &mut t.last_yield,
        57 => &mut t.last_rth_trade,
        78 => &mut t.creditman_mark_price,
        79 => &mut t.creditman_slow_mark_price,
        92 => &mut t.etf_nav_close,
        93 => &mut t.etf_nav_prior_close,
        94 => &mut t.etf_nav_bid,
        95 => &mut t.etf_nav_ask,
        96 => &mut t.etf_nav_last,
        97 => &mut t.etf_frozen_nav_last,
        98 => &mut t.etf_nav_high,
        99 => &mut t.etf_nav_low,
        101 => &mut t.estimated_ipo_midpoint,
        102 => &mut t.final_ipo_last,
        105 => &mut t.odd_lot_bid,
        106 => &mut t.odd_lot_ask,
        _ => return None,
    })
}

fn size_field(t: &mut Ticker, tick_type: i32) -> Option<&mut f64> {
    Some(match tick_type {
        8 | 74 => &mut t.volume,
        63 => &mut t.volume_rate3_min,
        64 => &mut t.volume_rate5_min,
        65 => &mut t.volume_rate10_min,
        21 => &mut t.av_volume,
        22 => &mut t.open_interest,
        27 => &mut t.call_open_interest,
        28 => &mut t.put_open_interest,
        29 => &mut t.call_volume,
        30 => &mut t.put_volume,
        34 => &mut t.auction_volume,
        36 => &mut t.auction_imbalance,
        61 => &mut t.regulatory_imbalance,
        86 => &mut t.futures_open_interest,
        87 => &mut t.av_option_volume,
        89 => &mut t.shortable_shares,
        107 => &mut t.odd_lot_bid_size,
        108 => &mut t.odd_lot_ask_size,
        _ => return None,
    })
}

fn generic_field(t: &mut Ticker, tick_type: i32) -> Option<&mut f64> {
    Some(match tick_type {
        23 => &mut t.hist_volatility,
        24 => &mut t.implied_volatility,
        31 => &mut t.index_future_premium,
        46 => &mut t.shortable,
        49 => &mut t.halted,
        54 => &mut t.trade_count,
        55 => &mut t.trade_rate,
        56 => &mut t.volume_rate,
        58 => &mut t.rt_hist_volatility,
        60 => &mut t.bond_factor_multiplier,
        90 => &mut t.delayed_halted,
        _ => return None,
    })
}

/// Greeks slot for an option computation tick type.
pub fn greeks_slot(t: &mut Ticker, tick_type: i32) -> Option<&mut Option<OptionComputation>> {
    Some(match tick_type {
        10 | 80 => &mut t.bid_greeks,
        11 | 81 => &mut t.ask_greeks,
        12 | 82 => &mut t.last_greeks,
        13 | 83 => &mut t.model_greeks,
        53 => &mut t.cust_greeks,
        _ => return None,
    })
}

fn efp_slot(t: &mut Ticker, tick_type: i32) -> Option<&mut Option<EfpData>> {
    Some(match tick_type {
        38 => &mut t.bid_efp,
        39 => &mut t.ask_efp,
        40 => &mut t.last_efp,
        41 => &mut t.open_efp,
        42 => &mut t.high_efp,
        43 => &mut t.low_efp,
        44 => &mut t.close_efp,
        _ => return None,
    })
}

impl Ticker {
    pub fn has_bid_ask(&self) -> bool {
        self.bid != -1.0
            && !self.bid.is_nan()
            && self.bid_size > 0.0
            && self.ask != -1.0
            && !self.ask.is_nan()
            && self.ask_size > 0.0
    }

    /// Mid point of bid and ask, NaN without a two-sided market.
    pub fn midpoint(&self) -> f64 {
        if self.has_bid_ask() {
            (self.bid + self.ask) * 0.5
        } else {
            f64::NAN
        }
    }

    /// Last price if it lies within the bid/ask spread, else the midpoint
    /// (ib_async `marketPrice`).
    pub fn market_price(&self) -> f64 {
        if self.has_bid_ask() {
            if self.bid <= self.last && self.last <= self.ask {
                self.last
            } else {
                self.midpoint()
            }
        } else {
            self.last
        }
    }

    pub(crate) fn clear_ticks(&mut self) {
        self.ticks.clear();
        self.tick_by_ticks.clear();
        self.dom_ticks.clear();
    }

    /// Apply a price tick with its implied size (ib_async `priceSizeTick`).
    pub fn on_price(
        &mut self,
        time: Timestamp,
        tick_type: i32,
        mut price: f64,
        mut size: f64,
        d: &Defaults,
    ) {
        match tick_type {
            1 | 66 => {
                if size == 0.0 {
                    price = d.empty_price;
                    size = d.empty_size;
                }
                self.prev_bid = self.bid;
                self.prev_bid_size = self.bid_size;
                self.bid = price;
                self.bid_size = size;
            }
            2 | 67 => {
                if size == 0.0 {
                    price = d.empty_price;
                    size = d.empty_size;
                }
                self.prev_ask = self.ask;
                self.prev_ask_size = self.ask_size;
                self.ask = price;
                self.ask_size = size;
            }
            4 | 68 => {
                if price == -1.0 && size == 0.0 && self.close > 0.0 {
                    price = d.empty_price;
                    size = d.empty_size;
                }
                self.prev_last = self.last;
                self.prev_last_size = self.last_size;
                self.last = price;
                self.last_size = size;
            }
            _ => match price_field(self, tick_type) {
                Some(f) => *f = price,
                None => tracing::debug!(tick_type, price, "price tick type has no ticker field"),
            },
        }
        if price != 0.0 || size != 0.0 {
            self.ticks.push(TickData {
                time,
                tick_type,
                price,
                size,
            });
        }
    }

    /// Apply a size tick (ib_async `tickSize`). Returns false when nothing changed.
    pub fn on_size(&mut self, time: Timestamp, tick_type: i32, size: f64, d: &Defaults) -> bool {
        let mut price = d.empty_price;
        match tick_type {
            0 | 69 => {
                if size == self.bid_size {
                    return false;
                }
                self.prev_bid_size = self.bid_size;
                if size == 0.0 {
                    self.bid = d.empty_price;
                    self.bid_size = d.empty_size;
                } else {
                    price = self.bid;
                    self.bid_size = size;
                }
            }
            3 | 70 => {
                if size == self.ask_size {
                    return false;
                }
                self.prev_ask_size = self.ask_size;
                if size == 0.0 {
                    self.ask = d.empty_price;
                    self.ask_size = d.empty_size;
                } else {
                    price = self.ask;
                    self.ask_size = size;
                }
            }
            5 | 71 => {
                price = self.last;
                if price.is_nan() {
                    return false;
                }
                if size != self.last_size {
                    self.prev_last_size = self.last_size;
                    self.last_size = size;
                }
            }
            _ => match size_field(self, tick_type) {
                Some(f) => *f = size,
                None => tracing::debug!(tick_type, size, "size tick type has no ticker field"),
            },
        }
        if price != 0.0 || size != 0.0 {
            self.ticks.push(TickData {
                time,
                tick_type,
                price,
                size,
            });
        }
        true
    }

    /// Apply a generic tick (ib_async `tickGeneric`).
    pub fn on_generic(&mut self, time: Timestamp, tick_type: i32, value: f64, d: &Defaults) {
        let value = if value > 0.0 { value } else { d.empty_size };
        match generic_field(self, tick_type) {
            Some(f) => *f = value,
            None => tracing::debug!(tick_type, value, "generic tick type has no ticker field"),
        }
        self.ticks.push(TickData {
            time,
            tick_type,
            price: value,
            size: 0.0,
        });
    }

    /// Apply a string tick (ib_async `tickString`).
    pub fn on_string(&mut self, time: Timestamp, tick_type: i32, value: &str) {
        match tick_type {
            25 => self.option_bid_exch = value.into(),
            26 => self.option_ask_exch = value.into(),
            32 => self.bid_exchange = value.into(),
            33 => self.ask_exchange = value.into(),
            84 => self.last_exchange = value.into(),
            85 => self.last_reg_time = value.into(),
            91 => self.reuters_mutual_funds = value.into(),
            100 => self.social_market_analytics = value.into(),
            109 => self.odd_lot_bid_exchange = value.into(),
            110 => self.odd_lot_ask_exchange = value.into(),
            45 | 88 => {
                if let Ok(ts) = value.parse::<i64>()
                    && ts != 0
                {
                    let slot = if tick_type == 45 {
                        &mut self.last_timestamp
                    } else {
                        &mut self.delayed_last_timestamp
                    };
                    *slot = Some(ts as f64);
                }
            }
            47 => {
                let mut ratios = HashMap::new();
                for kv in value.split(';').filter(|s| !s.is_empty()) {
                    if let Some((k, v)) = kv.split_once('=') {
                        let v = if v == "-99999.99" {
                            f64::NAN
                        } else {
                            v.parse().unwrap_or(f64::NAN)
                        };
                        ratios.insert(k.to_string(), v);
                    }
                }
                self.fundamental_ratios = Some(ratios);
            }
            48 | 77 => {
                // price;size;ms since epoch;total volume;VWAP;single trade
                let parts: Vec<&str> = value.split(';').collect();
                if parts.len() >= 5 {
                    if let Ok(v) = parts[3].parse::<f64>() {
                        if tick_type == 48 {
                            self.rt_volume = v;
                        } else {
                            self.rt_trade_volume = v;
                        }
                    }
                    if let Ok(v) = parts[4].parse::<f64>() {
                        self.vwap = v;
                    }
                    if let Ok(ms) = parts[2].parse::<f64>() {
                        self.rt_time = Some(ms / 1000.0);
                    }
                    if let (Ok(price), Ok(size)) =
                        (parts[0].parse::<f64>(), parts[1].parse::<f64>())
                    {
                        self.prev_last = self.last;
                        self.prev_last_size = self.last_size;
                        self.last = price;
                        self.last_size = size;
                        self.ticks.push(TickData {
                            time,
                            tick_type,
                            price,
                            size,
                        });
                    }
                }
            }
            59 => {
                let p: Vec<&str> = value.split(',').collect();
                if p.len() == 4 {
                    let num = |s: &str| if s.is_empty() { None } else { s.parse().ok() };
                    self.dividends = Some(Dividends {
                        past12_months: num(p[0]),
                        next12_months: num(p[1]),
                        next_date: if p[2].is_empty() {
                            None
                        } else {
                            Some(p[2].to_string())
                        },
                        next_amount: num(p[3]),
                    });
                }
            }
            _ => tracing::debug!(tick_type, value, "unhandled string tick"),
        }
    }

    pub fn on_efp(&mut self, tick_type: i32, efp: EfpData) {
        if let Some(slot) = efp_slot(self, tick_type) {
            *slot = Some(efp);
        }
    }

    /// Apply a market depth update (ib_async `updateMktDepthL2`).
    #[allow(clippy::too_many_arguments)]
    pub fn on_depth(
        &mut self,
        time: Timestamp,
        position: i32,
        market_maker: &str,
        operation: i32,
        side: i32,
        mut price: f64,
        mut size: f64,
    ) {
        let dom = if side != 0 {
            &mut self.dom_bids_dict
        } else {
            &mut self.dom_asks_dict
        };
        match operation {
            0 | 1 => {
                dom.insert(
                    position,
                    DOMLevel {
                        price,
                        size,
                        market_maker: market_maker.into(),
                    },
                );
            }
            2 => {
                size = 0.0;
                if let Some(level) = dom.remove(&position) {
                    price = level.price;
                }
            }
            _ => {}
        }
        let values: Vec<DOMLevel> = dom.values().cloned().collect();
        if side != 0 {
            self.dom_bids = values;
        } else {
            self.dom_asks = values;
        }
        self.dom_ticks.push(MktDepthData {
            time,
            position,
            market_maker: market_maker.into(),
            operation,
            side,
            price,
            size,
        });
    }

    /// Clear the order book (error 317, "market depth data has been reset").
    pub fn reset_depth(&mut self, time: Timestamp) {
        for l in &self.dom_asks {
            self.dom_ticks.push(MktDepthData {
                time,
                position: 0,
                market_maker: String::new(),
                operation: 2,
                side: 0,
                price: l.price,
                size: 0.0,
            });
        }
        for l in &self.dom_bids {
            self.dom_ticks.push(MktDepthData {
                time,
                position: 0,
                market_maker: String::new(),
                operation: 2,
                side: 1,
                price: l.price,
                size: 0.0,
            });
        }
        self.dom_asks.clear();
        self.dom_bids.clear();
        self.dom_asks_dict.clear();
        self.dom_bids_dict.clear();
    }

    pub fn on_tbt_all_last(
        &mut self,
        tick: TickByTick,
        mut price: f64,
        mut size: f64,
        d: &Defaults,
    ) {
        if price == -1.0 && size == 0.0 {
            price = d.empty_price;
            size = d.empty_size;
        }
        self.prev_last = self.last;
        self.prev_last_size = self.last_size;
        self.last = price;
        self.last_size = size;
        self.tick_by_ticks.push(tick);
    }

    pub fn on_tbt_bid_ask(
        &mut self,
        tick: TickByTick,
        bid: f64,
        ask: f64,
        bid_size: f64,
        ask_size: f64,
        d: &Defaults,
    ) {
        if bid != self.bid {
            self.prev_bid = self.bid;
            self.bid = if bid > 0.0 { bid } else { d.empty_price };
        }
        if bid_size != self.bid_size {
            self.prev_bid_size = self.bid_size;
            self.bid_size = if bid_size > 0.0 {
                bid_size
            } else {
                d.empty_size
            };
        }
        if ask != self.ask {
            self.prev_ask = self.ask;
            self.ask = if ask > 0.0 { ask } else { d.empty_price };
        }
        if ask_size != self.ask_size {
            self.prev_ask_size = self.ask_size;
            self.ask_size = if ask_size > 0.0 {
                ask_size
            } else {
                d.empty_size
            };
        }
        self.tick_by_ticks.push(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ibkit_tws::tick_type;

    #[test]
    fn odd_lot_ticks_fill_their_fields() {
        let d = Defaults::default();
        let mut t = Ticker::new(Contract::stock("AAPL", "SMART", "USD"));
        t.on_price(1.0, tick_type::ODD_LOT_BID, 150.1, 0.0, &d);
        t.on_price(1.0, tick_type::ODD_LOT_ASK, 150.2, 0.0, &d);
        t.on_size(1.0, tick_type::ODD_LOT_BID_SIZE, 30.0, &d);
        t.on_size(1.0, tick_type::ODD_LOT_ASK_SIZE, 40.0, &d);
        t.on_string(1.0, tick_type::ODD_LOT_BID_EXCH, "NYSE");
        t.on_string(1.0, tick_type::ODD_LOT_ASK_EXCH, "ARCA");
        assert_eq!(
            (
                t.odd_lot_bid,
                t.odd_lot_ask,
                t.odd_lot_bid_size,
                t.odd_lot_ask_size
            ),
            (150.1, 150.2, 30.0, 40.0)
        );
        assert_eq!(
            (
                t.odd_lot_bid_exchange.as_str(),
                t.odd_lot_ask_exchange.as_str()
            ),
            ("NYSE", "ARCA")
        );
        assert_eq!(tick_type::name(110), "ODD_LOT_ASK_EXCH");
        assert_eq!(tick_type::name(tick_type::NOT_SET), "NOT_SET");
    }
}
