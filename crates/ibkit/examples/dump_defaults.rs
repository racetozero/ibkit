//! Print the serde form of every default data type as JSON. The Python
//! package generator (`scripts/gen_python_types.py`) reads this output.

use ibkit::*;
use serde_json::{Map, json};

fn main() {
    let mut m = Map::new();
    macro_rules! dump {
        ($($name:literal => $v:expr),* $(,)?) => { $( m.insert($name.into(), serde_json::to_value($v).unwrap()); )* };
    }
    dump!(
        "Contract" => Contract::default(),
        "ComboLeg" => ComboLeg::default(),
        "DeltaNeutralContract" => DeltaNeutralContract::default(),
        "ContractDetails" => ContractDetails::default(),
        "ContractDescription" => ContractDescription::default(),
        "IneligibilityReason" => IneligibilityReason::default(),
        "Order" => Order::default(),
        "OrderState" => OrderState::default(),
        "OrderAllocation" => OrderAllocation::default(),
        "OrderComboLeg" => OrderComboLeg::default(),
        "OrderCancel" => OrderCancel::default(),
        "SoftDollarTier" => SoftDollarTier::default(),
        "Execution" => Execution::default(),
        "ExecutionFilter" => ExecutionFilter::default(),
        "CommissionReport" => CommissionReport::default(),
        "BarData" => BarData::default(),
        "RealTimeBar" => RealTimeBar::default(),
        "TickAttrib" => TickAttrib::default(),
        "TickAttribBidAsk" => TickAttribBidAsk::default(),
        "TickAttribLast" => TickAttribLast::default(),
        "HistoricalTick" => HistoricalTick::default(),
        "HistoricalTickBidAsk" => HistoricalTickBidAsk::default(),
        "HistoricalTickLast" => HistoricalTickLast::default(),
        "HistoricalSession" => HistoricalSession::default(),
        "HistoricalSchedule" => HistoricalSchedule::default(),
        "ScannerSubscription" => ScannerSubscription::default(),
        "ScanData" => ScanData::default(),
        "WshEventData" => WshEventData::default(),
        "FamilyCode" => FamilyCode::default(),
        "DepthMktDataDescription" => DepthMktDataDescription::default(),
        "SmartComponent" => SmartComponent::default(),
        "NewsProvider" => NewsProvider::default(),
        "HistogramData" => HistogramData::default(),
        "PriceIncrement" => PriceIncrement::default(),
        "AccountValue" => AccountValue::default(),
        "Position" => Position::default(),
        "PortfolioItem" => PortfolioItem::default(),
        "PnL" => PnL::new("", ""),
        "PnLSingle" => PnLSingle::new("", "", 0),
        "Fill" => Fill::default(),
        "TradeLogEntry" => TradeLogEntry::default(),
        "OrderStatus" => OrderStatus::default(),
        "TickData" => TickData::default(),
        "DOMLevel" => DOMLevel::default(),
        "MktDepthData" => MktDepthData::default(),
        "Dividends" => Dividends::default(),
        "OptionChain" => OptionChain::default(),
        "NewsTick" => NewsTick::default(),
        "NewsBulletin" => NewsBulletin::default(),
        "NewsArticle" => NewsArticle::default(),
        "HistoricalNews" => HistoricalNews::default(),
        "OptionComputation" => ibkit::tws::decode::OptionComputation::default(),
        "EfpData" => ibkit::tws::decode::EfpData::default(),
    );
    println!("{}", serde_json::to_string_pretty(&json!(m)).unwrap());
}
