"""Data classes generated from the Rust types. Do not edit by hand.

Regenerate with scripts/gen_python_types.py.
"""

from __future__ import annotations

import datetime as dt
from dataclasses import dataclass, field
from typing import Any, NamedTuple

UNSET_DOUBLE: float = 1.7976931348623157e308
UNSET_INTEGER: int = 2147483647
#: The default of a datetime field.
EPOCH: dt.datetime = dt.datetime(1970, 1, 1, tzinfo=dt.UTC)


class TagValue(NamedTuple):
    tag: str
    value: str


@dataclass
class OrderCondition:
    """Base of the order condition classes; see ``order.py``."""

    condType: int = 0
    conjunction: str = "a"


@dataclass
class Contract:
    conId: int = 0
    symbol: str = ''
    secType: str = ''
    lastTradeDateOrContractMonth: str = ''
    lastTradeDate: str = ''
    strike: float = 0.0
    right: str = ''
    multiplier: str = ''
    exchange: str = ''
    primaryExchange: str = ''
    currency: str = ''
    localSymbol: str = ''
    tradingClass: str = ''
    includeExpired: bool = False
    secIdType: str = ''
    secId: str = ''
    description: str = ''
    issuerId: str = ''
    comboLegsDescrip: str = ''
    comboLegs: list[ComboLeg] = field(default_factory=list)
    deltaNeutralContract: DeltaNeutralContract | None = None

@dataclass
class ComboLeg:
    conId: int = 0
    ratio: int = 0
    action: str = ''
    exchange: str = ''
    openClose: int = 0
    shortSaleSlot: int = 0
    designatedLocation: str = ''
    exemptCode: int = -1

@dataclass
class DeltaNeutralContract:
    conId: int = 0
    delta: float = 0.0
    price: float = 0.0

@dataclass
class ContractDetails:
    contract: Contract = field(default_factory=lambda: Contract())
    marketName: str = ''
    minTick: float = 0.0
    orderTypes: str = ''
    validExchanges: str = ''
    priceMagnifier: int = 0
    underConId: int = 0
    longName: str = ''
    contractMonth: str = ''
    industry: str = ''
    category: str = ''
    subcategory: str = ''
    timeZoneId: str = ''
    tradingHours: str = ''
    liquidHours: str = ''
    evRule: str = ''
    evMultiplier: int = 0
    aggGroup: int = 0
    underSymbol: str = ''
    underSecType: str = ''
    marketRuleIds: str = ''
    secIdList: list[TagValue] = field(default_factory=list)
    realExpirationDate: str = ''
    lastTradeTime: str = ''
    stockType: str = ''
    minSize: float = 0.0
    sizeIncrement: float = 0.0
    suggestedSizeIncrement: float = 0.0
    cusip: str = ''
    ratings: str = ''
    descAppend: str = ''
    bondType: str = ''
    couponType: str = ''
    callable: bool = False
    putable: bool = False
    coupon: float = 0.0
    convertible: bool = False
    maturity: str = ''
    issueDate: str = ''
    nextOptionDate: str = ''
    nextOptionType: str = ''
    nextOptionPartial: bool = False
    notes: str = ''
    fundName: str = ''
    fundFamily: str = ''
    fundType: str = ''
    fundFrontLoad: str = ''
    fundBackLoad: str = ''
    fundBackLoadTimeInterval: str = ''
    fundManagementFee: str = ''
    fundClosed: bool = False
    fundClosedForNewInvestors: bool = False
    fundClosedForNewMoney: bool = False
    fundNotifyAmount: str = ''
    fundMinimumInitialPurchase: str = ''
    fundSubsequentMinimumPurchase: str = ''
    fundBlueSkyStates: str = ''
    fundBlueSkyTerritories: str = ''
    fundDistributionPolicyIndicator: str = ''
    fundAssetType: str = ''
    ineligibilityReasonList: list[IneligibilityReason] = field(default_factory=list)
    eventContract1: str = ''
    eventContractDescription1: str = ''
    eventContractDescription2: str = ''
    minAlgoSize: float = 0.0
    lastPricePrecision: float = 0.0
    lastSizePrecision: float = 0.0
    settlementMethod: str = ''

@dataclass
class ContractDescription:
    contract: Contract = field(default_factory=lambda: Contract())
    derivativeSecTypes: list[Any] = field(default_factory=list)

@dataclass
class IneligibilityReason:
    id: str = ''
    description: str = ''

@dataclass
class Order:
    orderId: int = 0
    clientId: int = 0
    permId: int = 0
    action: str = ''
    totalQuantity: float = 0.0
    orderType: str = ''
    lmtPrice: float = UNSET_DOUBLE
    auxPrice: float = UNSET_DOUBLE
    tif: str = ''
    activeStartTime: str = ''
    activeStopTime: str = ''
    ocaGroup: str = ''
    ocaType: int = 0
    orderRef: str = ''
    transmit: bool = True
    parentId: int = 0
    blockOrder: bool = False
    sweepToFill: bool = False
    displaySize: int = 0
    triggerMethod: int = 0
    outsideRth: bool = False
    hidden: bool = False
    goodAfterTime: str = ''
    goodTillDate: str = ''
    rule80A: str = ''
    allOrNone: bool = False
    minQty: int = 2147483647
    percentOffset: float = UNSET_DOUBLE
    overridePercentageConstraints: bool = False
    trailStopPrice: float = UNSET_DOUBLE
    trailingPercent: float = UNSET_DOUBLE
    faGroup: str = ''
    faProfile: str = ''
    faMethod: str = ''
    faPercentage: str = ''
    designatedLocation: str = ''
    openClose: str = 'O'
    origin: int = 0
    shortSaleSlot: int = 0
    exemptCode: int = -1
    discretionaryAmt: float = 0.0
    eTradeOnly: bool = False
    firmQuoteOnly: bool = False
    nbboPriceCap: float = UNSET_DOUBLE
    optOutSmartRouting: bool = False
    auctionStrategy: int = 0
    startingPrice: float = UNSET_DOUBLE
    stockRefPrice: float = UNSET_DOUBLE
    delta: float = UNSET_DOUBLE
    stockRangeLower: float = UNSET_DOUBLE
    stockRangeUpper: float = UNSET_DOUBLE
    randomizePrice: bool = False
    randomizeSize: bool = False
    volatility: float = UNSET_DOUBLE
    volatilityType: int = 2147483647
    deltaNeutralOrderType: str = ''
    deltaNeutralAuxPrice: float = UNSET_DOUBLE
    deltaNeutralConId: int = 0
    deltaNeutralSettlingFirm: str = ''
    deltaNeutralClearingAccount: str = ''
    deltaNeutralClearingIntent: str = ''
    deltaNeutralOpenClose: str = ''
    deltaNeutralShortSale: bool = False
    deltaNeutralShortSaleSlot: int = 0
    deltaNeutralDesignatedLocation: str = ''
    continuousUpdate: bool = False
    referencePriceType: int = 2147483647
    basisPoints: float = UNSET_DOUBLE
    basisPointsType: int = 2147483647
    scaleInitLevelSize: int = 2147483647
    scaleSubsLevelSize: int = 2147483647
    scalePriceIncrement: float = UNSET_DOUBLE
    scalePriceAdjustValue: float = UNSET_DOUBLE
    scalePriceAdjustInterval: int = 2147483647
    scaleProfitOffset: float = UNSET_DOUBLE
    scaleAutoReset: bool = False
    scaleInitPosition: int = 2147483647
    scaleInitFillQty: int = 2147483647
    scaleRandomPercent: bool = False
    scaleTable: str = ''
    hedgeType: str = ''
    hedgeParam: str = ''
    hedgeMaxSize: int = 2147483647
    account: str = ''
    settlingFirm: str = ''
    clearingAccount: str = ''
    clearingIntent: str = ''
    algoStrategy: str = ''
    algoParams: list[TagValue] = field(default_factory=list)
    smartComboRoutingParams: list[TagValue] = field(default_factory=list)
    algoId: str = ''
    whatIf: bool = False
    notHeld: bool = False
    solicited: bool = False
    modelCode: str = ''
    orderComboLegs: list[OrderComboLeg] = field(default_factory=list)
    orderMiscOptions: list[TagValue] = field(default_factory=list)
    referenceContractId: int = 0
    peggedChangeAmount: float = 0.0
    isPeggedChangeAmountDecrease: bool = False
    referenceChangeAmount: float = 0.0
    referenceExchangeId: str = ''
    adjustedOrderType: str = ''
    triggerPrice: float = UNSET_DOUBLE
    adjustedStopPrice: float = UNSET_DOUBLE
    adjustedStopLimitPrice: float = UNSET_DOUBLE
    adjustedTrailingAmount: float = UNSET_DOUBLE
    adjustableTrailingUnit: int = 0
    lmtPriceOffset: float = UNSET_DOUBLE
    conditions: list[OrderCondition] = field(default_factory=list)
    conditionsCancelOrder: bool = False
    conditionsIgnoreRth: bool = False
    conditionsIncludeOvernight: bool = False
    extOperator: str = ''
    softDollarTier: SoftDollarTier = field(default_factory=lambda: SoftDollarTier())
    cashQty: float = UNSET_DOUBLE
    mifid2DecisionMaker: str = ''
    mifid2DecisionAlgo: str = ''
    mifid2ExecutionTrader: str = ''
    mifid2ExecutionAlgo: str = ''
    dontUseAutoPriceForHedge: bool = False
    isOmsContainer: bool = False
    discretionaryUpToLimitPrice: bool = False
    autoCancelDate: str = ''
    filledQuantity: float = UNSET_DOUBLE
    refFuturesConId: int = 0
    autoCancelParent: bool = False
    shareholder: str = ''
    imbalanceOnly: bool = False
    routeMarketableToBbo: Any = None
    parentPermId: int = 0
    usePriceMgmtAlgo: Any = None
    duration: int = 2147483647
    postToAts: int = 2147483647
    advancedErrorOverride: str = ''
    manualOrderTime: str = ''
    minTradeQty: int = 2147483647
    minCompeteSize: int = 2147483647
    competeAgainstBestOffset: float = UNSET_DOUBLE
    midOffsetAtWhole: float = UNSET_DOUBLE
    midOffsetAtHalf: float = UNSET_DOUBLE
    customerAccount: str = ''
    professionalCustomer: bool = False
    bondAccruedInterest: str = ''
    includeOvernight: bool = False
    manualOrderIndicator: int = 2147483647
    submitter: str = ''
    postOnly: bool = False
    allowPreOpen: bool = False
    ignoreOpenAuction: bool = False
    deactivate: bool = False
    seekPriceImprovement: Any = None
    whatIfType: int = 2147483647
    slOrderId: int = 2147483647
    slOrderType: str = ''
    ptOrderId: int = 2147483647
    ptOrderType: str = ''

@dataclass
class OrderState:
    status: str = ''
    initMarginBefore: str = ''
    maintMarginBefore: str = ''
    equityWithLoanBefore: str = ''
    initMarginChange: str = ''
    maintMarginChange: str = ''
    equityWithLoanChange: str = ''
    initMarginAfter: str = ''
    maintMarginAfter: str = ''
    equityWithLoanAfter: str = ''
    commission: float = UNSET_DOUBLE
    minCommission: float = UNSET_DOUBLE
    maxCommission: float = UNSET_DOUBLE
    commissionCurrency: str = ''
    marginCurrency: str = ''
    initMarginBeforeOutsideRth: float = UNSET_DOUBLE
    maintMarginBeforeOutsideRth: float = UNSET_DOUBLE
    equityWithLoanBeforeOutsideRth: float = UNSET_DOUBLE
    initMarginChangeOutsideRth: float = UNSET_DOUBLE
    maintMarginChangeOutsideRth: float = UNSET_DOUBLE
    equityWithLoanChangeOutsideRth: float = UNSET_DOUBLE
    initMarginAfterOutsideRth: float = UNSET_DOUBLE
    maintMarginAfterOutsideRth: float = UNSET_DOUBLE
    equityWithLoanAfterOutsideRth: float = UNSET_DOUBLE
    suggestedSize: float = UNSET_DOUBLE
    rejectReason: str = ''
    orderAllocations: list[OrderAllocation] = field(default_factory=list)
    warningText: str = ''
    completedTime: str = ''
    completedStatus: str = ''

@dataclass
class OrderAllocation:
    account: str = ''
    position: float = 0.0
    positionDesired: float = 0.0
    positionAfter: float = 0.0
    desiredAllocQty: float = 0.0
    allowedAllocQty: float = 0.0
    isMonetary: bool = False

@dataclass
class OrderComboLeg:
    price: float = UNSET_DOUBLE

@dataclass
class OrderCancel:
    manualOrderCancelTime: str = ''
    extOperator: str = ''
    manualOrderIndicator: int = 2147483647

@dataclass
class SoftDollarTier:
    name: str = ''
    val: str = ''
    displayName: str = ''

@dataclass
class Execution:
    execId: str = ''
    time: dt.datetime = EPOCH
    acctNumber: str = ''
    exchange: str = ''
    side: str = ''
    shares: float = 0.0
    price: float = 0.0
    permId: int = 0
    clientId: int = 0
    orderId: int = 0
    liquidation: int = 0
    cumQty: float = 0.0
    avgPrice: float = 0.0
    orderRef: str = ''
    evRule: str = ''
    evMultiplier: float = 0.0
    modelCode: str = ''
    lastLiquidity: int = 0
    pendingPriceRevision: bool = False
    submitter: str = ''
    optExerciseOrLapseType: int = 0

@dataclass
class ExecutionFilter:
    clientId: int = 0
    acctCode: str = ''
    time: str = ''
    symbol: str = ''
    secType: str = ''
    exchange: str = ''
    side: str = ''
    lastNDays: int = 2147483647
    specificDates: list[Any] = field(default_factory=list)

@dataclass
class CommissionReport:
    execId: str = ''
    commission: float = 0.0
    currency: str = ''
    realizedPNL: float = 0.0
    yield_: float = 0.0
    yieldRedemptionDate: int = 0

@dataclass
class BarData:
    date: str = ''
    open: float = 0.0
    high: float = 0.0
    low: float = 0.0
    close: float = 0.0
    volume: float = 0.0
    average: float = 0.0
    barCount: int = 0

@dataclass
class RealTimeBar:
    time: dt.datetime = EPOCH
    open_: float = 0.0
    high: float = 0.0
    low: float = 0.0
    close: float = 0.0
    volume: float = 0.0
    wap: float = 0.0
    count: int = 0

@dataclass
class TickAttrib:
    canAutoExecute: bool = False
    pastLimit: bool = False
    preOpen: bool = False

@dataclass
class TickAttribBidAsk:
    bidPastLow: bool = False
    askPastHigh: bool = False

@dataclass
class TickAttribLast:
    pastLimit: bool = False
    unreported: bool = False

@dataclass
class HistoricalTick:
    time: dt.datetime = EPOCH
    price: float = 0.0
    size: float = 0.0

@dataclass
class HistoricalTickBidAsk:
    time: dt.datetime = EPOCH
    tickAttribBidAsk: TickAttribBidAsk = field(default_factory=lambda: TickAttribBidAsk())
    priceBid: float = 0.0
    priceAsk: float = 0.0
    sizeBid: float = 0.0
    sizeAsk: float = 0.0

@dataclass
class HistoricalTickLast:
    time: dt.datetime = EPOCH
    tickAttribLast: TickAttribLast = field(default_factory=lambda: TickAttribLast())
    price: float = 0.0
    size: float = 0.0
    exchange: str = ''
    specialConditions: str = ''

@dataclass
class HistoricalSession:
    startDateTime: str = ''
    endDateTime: str = ''
    refDate: str = ''

@dataclass
class HistoricalSchedule:
    startDateTime: str = ''
    endDateTime: str = ''
    timeZone: str = ''
    sessions: list[HistoricalSession] = field(default_factory=list)

@dataclass
class ScannerSubscription:
    numberOfRows: int = -1
    instrument: str = ''
    locationCode: str = ''
    scanCode: str = ''
    abovePrice: float = UNSET_DOUBLE
    belowPrice: float = UNSET_DOUBLE
    aboveVolume: int = 2147483647
    marketCapAbove: float = UNSET_DOUBLE
    marketCapBelow: float = UNSET_DOUBLE
    moodyRatingAbove: str = ''
    moodyRatingBelow: str = ''
    spRatingAbove: str = ''
    spRatingBelow: str = ''
    maturityDateAbove: str = ''
    maturityDateBelow: str = ''
    couponRateAbove: float = UNSET_DOUBLE
    couponRateBelow: float = UNSET_DOUBLE
    excludeConvertible: bool = False
    averageOptionVolumeAbove: int = 2147483647
    scannerSettingPairs: str = ''
    stockTypeFilter: str = ''

@dataclass
class ScanData:
    rank: int = 0
    contractDetails: ContractDetails = field(default_factory=lambda: ContractDetails())
    distance: str = ''
    benchmark: str = ''
    projection: str = ''
    legsStr: str = ''

@dataclass
class WshEventData:
    conId: int = 2147483647
    filter: str = ''
    fillWatchlist: bool = False
    fillPortfolio: bool = False
    fillCompetitors: bool = False
    startDate: str = ''
    endDate: str = ''
    totalLimit: int = 2147483647

@dataclass
class FamilyCode:
    accountID: str = ''
    familyCodeStr: str = ''

@dataclass
class DepthMktDataDescription:
    exchange: str = ''
    secType: str = ''
    listingExch: str = ''
    serviceDataType: str = ''
    aggGroup: int = 2147483647

@dataclass
class SmartComponent:
    bitNumber: int = 0
    exchange: str = ''
    exchangeLetter: str = ''

@dataclass
class NewsProvider:
    code: str = ''
    name: str = ''

@dataclass
class HistogramData:
    price: float = 0.0
    count: float = 0.0

@dataclass
class PriceIncrement:
    lowEdge: float = 0.0
    increment: float = 0.0

@dataclass
class AccountValue:
    account: str = ''
    tag: str = ''
    value: str = ''
    currency: str = ''
    modelCode: str = ''

@dataclass
class Position:
    account: str = ''
    contract: Contract = field(default_factory=lambda: Contract())
    position: float = 0.0
    avgCost: float = 0.0

@dataclass
class PortfolioItem:
    contract: Contract = field(default_factory=lambda: Contract())
    position: float = 0.0
    marketPrice: float = 0.0
    marketValue: float = 0.0
    averageCost: float = 0.0
    unrealizedPNL: float = 0.0
    realizedPNL: float = 0.0
    account: str = ''

@dataclass
class PnL:
    account: str = ''
    modelCode: str = ''
    dailyPnL: Any = None
    unrealizedPnL: Any = None
    realizedPnL: Any = None

@dataclass
class PnLSingle:
    account: str = ''
    modelCode: str = ''
    conId: int = 0
    dailyPnL: Any = None
    unrealizedPnL: Any = None
    realizedPnL: Any = None
    position: float = 0.0
    value: Any = None

@dataclass
class Fill:
    contract: Contract = field(default_factory=lambda: Contract())
    execution: Execution = field(default_factory=lambda: Execution())
    commissionReport: CommissionReport = field(default_factory=lambda: CommissionReport())
    time: dt.datetime = EPOCH

@dataclass
class TradeLogEntry:
    time: dt.datetime = EPOCH
    status: str = ''
    message: str = ''
    errorCode: int = 0

@dataclass
class OrderStatus:
    orderId: int = 0
    status: str = ''
    filled: float = 0.0
    remaining: float = 0.0
    avgFillPrice: float = 0.0
    permId: int = 0
    parentId: int = 0
    lastFillPrice: float = 0.0
    clientId: int = 0
    whyHeld: str = ''
    mktCapPrice: float = 0.0

@dataclass
class TickData:
    time: dt.datetime = EPOCH
    tickType: int = 0
    price: float = 0.0
    size: float = 0.0

@dataclass
class DOMLevel:
    price: float = 0.0
    size: float = 0.0
    marketMaker: str = ''

@dataclass
class MktDepthData:
    time: dt.datetime = EPOCH
    position: int = 0
    marketMaker: str = ''
    operation: int = 0
    side: int = 0
    price: float = 0.0
    size: float = 0.0

@dataclass
class Dividends:
    past12Months: Any = None
    next12Months: Any = None
    nextDate: Any = None
    nextAmount: Any = None

@dataclass
class OptionChain:
    exchange: str = ''
    underlyingConId: int = 0
    tradingClass: str = ''
    multiplier: str = ''
    expirations: list[Any] = field(default_factory=list)
    strikes: list[Any] = field(default_factory=list)

@dataclass
class NewsTick:
    timeStamp: int = 0
    providerCode: str = ''
    articleId: str = ''
    headline: str = ''
    extraData: str = ''

@dataclass
class NewsBulletin:
    msgId: int = 0
    msgType: int = 0
    message: str = ''
    origExchange: str = ''

@dataclass
class NewsArticle:
    articleType: int = 0
    articleText: str = ''

@dataclass
class HistoricalNews:
    time: str = ''
    providerCode: str = ''
    articleId: str = ''
    headline: str = ''

@dataclass
class OptionComputation:
    tickAttrib: Any = None
    impliedVol: Any = None
    delta: Any = None
    optPrice: Any = None
    pvDividend: Any = None
    gamma: Any = None
    vega: Any = None
    theta: Any = None
    undPrice: Any = None

@dataclass
class EfpData:
    basisPoints: float = 0.0
    formattedBasisPoints: str = ''
    impliedFuture: float = 0.0
    holdDays: int = 0
    futureLastTradeDate: str = ''
    dividendImpact: float = 0.0
    dividendsToLastTradeDate: float = 0.0

# Class name -> {field: class of nested object}.
NESTED: dict[str, dict[str, str]] = {'Contract': {'deltaNeutralContract': 'DeltaNeutralContract'}, 'ComboLeg': {}, 'DeltaNeutralContract': {}, 'ContractDetails': {'contract': 'Contract'}, 'ContractDescription': {'contract': 'Contract'}, 'IneligibilityReason': {}, 'Order': {'softDollarTier': 'SoftDollarTier'}, 'OrderState': {}, 'OrderAllocation': {}, 'OrderComboLeg': {}, 'OrderCancel': {}, 'SoftDollarTier': {}, 'Execution': {}, 'ExecutionFilter': {}, 'CommissionReport': {}, 'BarData': {}, 'RealTimeBar': {}, 'TickAttrib': {}, 'TickAttribBidAsk': {}, 'TickAttribLast': {}, 'HistoricalTick': {}, 'HistoricalTickBidAsk': {'tickAttribBidAsk': 'TickAttribBidAsk'}, 'HistoricalTickLast': {'tickAttribLast': 'TickAttribLast'}, 'HistoricalSession': {}, 'HistoricalSchedule': {}, 'ScannerSubscription': {}, 'ScanData': {'contractDetails': 'ContractDetails'}, 'WshEventData': {}, 'FamilyCode': {}, 'DepthMktDataDescription': {}, 'SmartComponent': {}, 'NewsProvider': {}, 'HistogramData': {}, 'PriceIncrement': {}, 'AccountValue': {}, 'Position': {'contract': 'Contract'}, 'PortfolioItem': {'contract': 'Contract'}, 'PnL': {}, 'PnLSingle': {}, 'Fill': {'contract': 'Contract', 'execution': 'Execution', 'commissionReport': 'CommissionReport'}, 'TradeLogEntry': {}, 'OrderStatus': {}, 'TickData': {}, 'DOMLevel': {}, 'MktDepthData': {}, 'Dividends': {}, 'OptionChain': {}, 'NewsTick': {}, 'NewsBulletin': {}, 'NewsArticle': {}, 'HistoricalNews': {}, 'OptionComputation': {}, 'EfpData': {}}
# Class name -> {field: class of list elements}.
LISTS: dict[str, dict[str, str]] = {'Contract': {'comboLegs': 'ComboLeg'}, 'ComboLeg': {}, 'DeltaNeutralContract': {}, 'ContractDetails': {'secIdList': 'TagValue', 'ineligibilityReasonList': 'IneligibilityReason'}, 'ContractDescription': {}, 'IneligibilityReason': {}, 'Order': {'algoParams': 'TagValue', 'smartComboRoutingParams': 'TagValue', 'orderComboLegs': 'OrderComboLeg', 'orderMiscOptions': 'TagValue', 'conditions': 'OrderCondition'}, 'OrderState': {'orderAllocations': 'OrderAllocation'}, 'OrderAllocation': {}, 'OrderComboLeg': {}, 'OrderCancel': {}, 'SoftDollarTier': {}, 'Execution': {}, 'ExecutionFilter': {}, 'CommissionReport': {}, 'BarData': {}, 'RealTimeBar': {}, 'TickAttrib': {}, 'TickAttribBidAsk': {}, 'TickAttribLast': {}, 'HistoricalTick': {}, 'HistoricalTickBidAsk': {}, 'HistoricalTickLast': {}, 'HistoricalSession': {}, 'HistoricalSchedule': {'sessions': 'HistoricalSession'}, 'ScannerSubscription': {}, 'ScanData': {}, 'WshEventData': {}, 'FamilyCode': {}, 'DepthMktDataDescription': {}, 'SmartComponent': {}, 'NewsProvider': {}, 'HistogramData': {}, 'PriceIncrement': {}, 'AccountValue': {}, 'Position': {}, 'PortfolioItem': {}, 'PnL': {}, 'PnLSingle': {}, 'Fill': {}, 'TradeLogEntry': {}, 'OrderStatus': {}, 'TickData': {}, 'DOMLevel': {}, 'MktDepthData': {}, 'Dividends': {}, 'OptionChain': {}, 'NewsTick': {}, 'NewsBulletin': {}, 'NewsArticle': {}, 'HistoricalNews': {}, 'OptionComputation': {}, 'EfpData': {}}
GENERATED: tuple[str, ...] = ('Contract', 'ComboLeg', 'DeltaNeutralContract', 'ContractDetails', 'ContractDescription', 'IneligibilityReason', 'Order', 'OrderState', 'OrderAllocation', 'OrderComboLeg', 'OrderCancel', 'SoftDollarTier', 'Execution', 'ExecutionFilter', 'CommissionReport', 'BarData', 'RealTimeBar', 'TickAttrib', 'TickAttribBidAsk', 'TickAttribLast', 'HistoricalTick', 'HistoricalTickBidAsk', 'HistoricalTickLast', 'HistoricalSession', 'HistoricalSchedule', 'ScannerSubscription', 'ScanData', 'WshEventData', 'FamilyCode', 'DepthMktDataDescription', 'SmartComponent', 'NewsProvider', 'HistogramData', 'PriceIncrement', 'AccountValue', 'Position', 'PortfolioItem', 'PnL', 'PnLSingle', 'Fill', 'TradeLogEntry', 'OrderStatus', 'TickData', 'DOMLevel', 'MktDepthData', 'Dividends', 'OptionChain', 'NewsTick', 'NewsBulletin', 'NewsArticle', 'HistoricalNews', 'OptionComputation', 'EfpData')
__all__ = ['EPOCH', 'UNSET_DOUBLE', 'UNSET_INTEGER', 'AccountValue', 'BarData', 'ComboLeg', 'CommissionReport', 'Contract', 'ContractDescription', 'ContractDetails', 'DOMLevel', 'DeltaNeutralContract', 'DepthMktDataDescription', 'Dividends', 'EfpData', 'Execution', 'ExecutionFilter', 'FamilyCode', 'Fill', 'HistogramData', 'HistoricalNews', 'HistoricalSchedule', 'HistoricalSession', 'HistoricalTick', 'HistoricalTickBidAsk', 'HistoricalTickLast', 'IneligibilityReason', 'MktDepthData', 'NewsArticle', 'NewsBulletin', 'NewsProvider', 'NewsTick', 'OptionChain', 'OptionComputation', 'Order', 'OrderAllocation', 'OrderCancel', 'OrderComboLeg', 'OrderState', 'OrderStatus', 'PnL', 'PnLSingle', 'PortfolioItem', 'Position', 'PriceIncrement', 'RealTimeBar', 'ScanData', 'ScannerSubscription', 'SmartComponent', 'SoftDollarTier', 'TagValue', 'TickAttrib', 'TickAttribBidAsk', 'TickAttribLast', 'TickData', 'TradeLogEntry', 'WshEventData']
