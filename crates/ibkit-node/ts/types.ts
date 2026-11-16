// Data types generated from the Rust types by scripts/gen_python_types.py.
// Do not edit by hand. Inputs accept any subset of the fields (the Rust
// side fills the rest with the defaults in DEFAULTS).

export const UNSET_DOUBLE = 1.7976931348623157e308;
export const UNSET_INTEGER = 2147483647;

export interface TagValue {
  tag: string;
  value: string;
}

/** An order condition; `condType` selects the kind (see `order.ts`). */
export interface OrderCondition {
  condType: string;
  conjunction: string;
  [field: string]: unknown;
}

export interface Contract {
  conId: number;
  symbol: string;
  secType: string;
  lastTradeDateOrContractMonth: string;
  lastTradeDate: string;
  strike: number;
  right: string;
  multiplier: string;
  exchange: string;
  primaryExchange: string;
  currency: string;
  localSymbol: string;
  tradingClass: string;
  includeExpired: boolean;
  secIdType: string;
  secId: string;
  description: string;
  issuerId: string;
  comboLegsDescrip: string;
  comboLegs: ComboLeg[];
  deltaNeutralContract: DeltaNeutralContract | null;
}

export interface ComboLeg {
  conId: number;
  ratio: number;
  action: string;
  exchange: string;
  openClose: number;
  shortSaleSlot: number;
  designatedLocation: string;
  exemptCode: number;
}

export interface DeltaNeutralContract {
  conId: number;
  delta: number;
  price: number;
}

export interface ContractDetails {
  contract: Contract;
  marketName: string;
  minTick: number;
  orderTypes: string;
  validExchanges: string;
  priceMagnifier: number;
  underConId: number;
  longName: string;
  contractMonth: string;
  industry: string;
  category: string;
  subcategory: string;
  timeZoneId: string;
  tradingHours: string;
  liquidHours: string;
  evRule: string;
  evMultiplier: number;
  aggGroup: number;
  underSymbol: string;
  underSecType: string;
  marketRuleIds: string;
  secIdList: TagValue[];
  realExpirationDate: string;
  lastTradeTime: string;
  stockType: string;
  minSize: number;
  sizeIncrement: number;
  suggestedSizeIncrement: number;
  cusip: string;
  ratings: string;
  descAppend: string;
  bondType: string;
  couponType: string;
  callable: boolean;
  putable: boolean;
  coupon: number;
  convertible: boolean;
  maturity: string;
  issueDate: string;
  nextOptionDate: string;
  nextOptionType: string;
  nextOptionPartial: boolean;
  notes: string;
  fundName: string;
  fundFamily: string;
  fundType: string;
  fundFrontLoad: string;
  fundBackLoad: string;
  fundBackLoadTimeInterval: string;
  fundManagementFee: string;
  fundClosed: boolean;
  fundClosedForNewInvestors: boolean;
  fundClosedForNewMoney: boolean;
  fundNotifyAmount: string;
  fundMinimumInitialPurchase: string;
  fundSubsequentMinimumPurchase: string;
  fundBlueSkyStates: string;
  fundBlueSkyTerritories: string;
  fundDistributionPolicyIndicator: string;
  fundAssetType: string;
  ineligibilityReasonList: IneligibilityReason[];
  eventContract1: string;
  eventContractDescription1: string;
  eventContractDescription2: string;
  minAlgoSize: number;
  lastPricePrecision: number;
  lastSizePrecision: number;
  settlementMethod: string;
}

export interface ContractDescription {
  contract: Contract;
  derivativeSecTypes: unknown[];
}

export interface IneligibilityReason {
  id: string;
  description: string;
}

export interface Order {
  orderId: number;
  clientId: number;
  permId: number;
  action: string;
  totalQuantity: number;
  orderType: string;
  lmtPrice: number;
  auxPrice: number;
  tif: string;
  activeStartTime: string;
  activeStopTime: string;
  ocaGroup: string;
  ocaType: number;
  orderRef: string;
  transmit: boolean;
  parentId: number;
  blockOrder: boolean;
  sweepToFill: boolean;
  displaySize: number;
  triggerMethod: number;
  outsideRth: boolean;
  hidden: boolean;
  goodAfterTime: string;
  goodTillDate: string;
  rule80A: string;
  allOrNone: boolean;
  minQty: number;
  percentOffset: number;
  overridePercentageConstraints: boolean;
  trailStopPrice: number;
  trailingPercent: number;
  faGroup: string;
  faProfile: string;
  faMethod: string;
  faPercentage: string;
  designatedLocation: string;
  openClose: string;
  origin: number;
  shortSaleSlot: number;
  exemptCode: number;
  discretionaryAmt: number;
  eTradeOnly: boolean;
  firmQuoteOnly: boolean;
  nbboPriceCap: number;
  optOutSmartRouting: boolean;
  auctionStrategy: number;
  startingPrice: number;
  stockRefPrice: number;
  delta: number;
  stockRangeLower: number;
  stockRangeUpper: number;
  randomizePrice: boolean;
  randomizeSize: boolean;
  volatility: number;
  volatilityType: number;
  deltaNeutralOrderType: string;
  deltaNeutralAuxPrice: number;
  deltaNeutralConId: number;
  deltaNeutralSettlingFirm: string;
  deltaNeutralClearingAccount: string;
  deltaNeutralClearingIntent: string;
  deltaNeutralOpenClose: string;
  deltaNeutralShortSale: boolean;
  deltaNeutralShortSaleSlot: number;
  deltaNeutralDesignatedLocation: string;
  continuousUpdate: boolean;
  referencePriceType: number;
  basisPoints: number;
  basisPointsType: number;
  scaleInitLevelSize: number;
  scaleSubsLevelSize: number;
  scalePriceIncrement: number;
  scalePriceAdjustValue: number;
  scalePriceAdjustInterval: number;
  scaleProfitOffset: number;
  scaleAutoReset: boolean;
  scaleInitPosition: number;
  scaleInitFillQty: number;
  scaleRandomPercent: boolean;
  scaleTable: string;
  hedgeType: string;
  hedgeParam: string;
  hedgeMaxSize: number;
  account: string;
  settlingFirm: string;
  clearingAccount: string;
  clearingIntent: string;
  algoStrategy: string;
  algoParams: TagValue[];
  smartComboRoutingParams: TagValue[];
  algoId: string;
  whatIf: boolean;
  notHeld: boolean;
  solicited: boolean;
  modelCode: string;
  orderComboLegs: OrderComboLeg[];
  orderMiscOptions: TagValue[];
  referenceContractId: number;
  peggedChangeAmount: number;
  isPeggedChangeAmountDecrease: boolean;
  referenceChangeAmount: number;
  referenceExchangeId: string;
  adjustedOrderType: string;
  triggerPrice: number;
  adjustedStopPrice: number;
  adjustedStopLimitPrice: number;
  adjustedTrailingAmount: number;
  adjustableTrailingUnit: number;
  lmtPriceOffset: number;
  conditions: OrderCondition[];
  conditionsCancelOrder: boolean;
  conditionsIgnoreRth: boolean;
  conditionsIncludeOvernight: boolean;
  extOperator: string;
  softDollarTier: SoftDollarTier;
  cashQty: number;
  mifid2DecisionMaker: string;
  mifid2DecisionAlgo: string;
  mifid2ExecutionTrader: string;
  mifid2ExecutionAlgo: string;
  dontUseAutoPriceForHedge: boolean;
  isOmsContainer: boolean;
  discretionaryUpToLimitPrice: boolean;
  autoCancelDate: string;
  filledQuantity: number;
  refFuturesConId: number;
  autoCancelParent: boolean;
  shareholder: string;
  imbalanceOnly: boolean;
  routeMarketableToBbo: unknown;
  parentPermId: number;
  usePriceMgmtAlgo: unknown;
  duration: number;
  postToAts: number;
  advancedErrorOverride: string;
  manualOrderTime: string;
  minTradeQty: number;
  minCompeteSize: number;
  competeAgainstBestOffset: number;
  midOffsetAtWhole: number;
  midOffsetAtHalf: number;
  customerAccount: string;
  professionalCustomer: boolean;
  bondAccruedInterest: string;
  includeOvernight: boolean;
  manualOrderIndicator: number;
  submitter: string;
  postOnly: boolean;
  allowPreOpen: boolean;
  ignoreOpenAuction: boolean;
  deactivate: boolean;
  seekPriceImprovement: unknown;
  whatIfType: number;
  slOrderId: number;
  slOrderType: string;
  ptOrderId: number;
  ptOrderType: string;
}

export interface OrderState {
  status: string;
  initMarginBefore: string;
  maintMarginBefore: string;
  equityWithLoanBefore: string;
  initMarginChange: string;
  maintMarginChange: string;
  equityWithLoanChange: string;
  initMarginAfter: string;
  maintMarginAfter: string;
  equityWithLoanAfter: string;
  commission: number;
  minCommission: number;
  maxCommission: number;
  commissionCurrency: string;
  marginCurrency: string;
  initMarginBeforeOutsideRth: number;
  maintMarginBeforeOutsideRth: number;
  equityWithLoanBeforeOutsideRth: number;
  initMarginChangeOutsideRth: number;
  maintMarginChangeOutsideRth: number;
  equityWithLoanChangeOutsideRth: number;
  initMarginAfterOutsideRth: number;
  maintMarginAfterOutsideRth: number;
  equityWithLoanAfterOutsideRth: number;
  suggestedSize: number;
  rejectReason: string;
  orderAllocations: OrderAllocation[];
  warningText: string;
  completedTime: string;
  completedStatus: string;
}

export interface OrderAllocation {
  account: string;
  position: number;
  positionDesired: number;
  positionAfter: number;
  desiredAllocQty: number;
  allowedAllocQty: number;
  isMonetary: boolean;
}

export interface OrderComboLeg {
  price: number;
}

export interface OrderCancel {
  manualOrderCancelTime: string;
  extOperator: string;
  manualOrderIndicator: number;
}

export interface SoftDollarTier {
  name: string;
  val: string;
  displayName: string;
}

export interface Execution {
  execId: string;
  time: string;
  acctNumber: string;
  exchange: string;
  side: string;
  shares: number;
  price: number;
  permId: number;
  clientId: number;
  orderId: number;
  liquidation: number;
  cumQty: number;
  avgPrice: number;
  orderRef: string;
  evRule: string;
  evMultiplier: number;
  modelCode: string;
  lastLiquidity: number;
  pendingPriceRevision: boolean;
  submitter: string;
  optExerciseOrLapseType: number;
}

export interface ExecutionFilter {
  clientId: number;
  acctCode: string;
  time: string;
  symbol: string;
  secType: string;
  exchange: string;
  side: string;
  lastNDays: number;
  specificDates: unknown[];
}

export interface CommissionReport {
  execId: string;
  commission: number;
  currency: string;
  realizedPNL: number;
  yield_: number;
  yieldRedemptionDate: number;
}

export interface BarData {
  date: string;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  average: number;
  barCount: number;
}

export interface RealTimeBar {
  time: number;
  open_: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  wap: number;
  count: number;
}

export interface TickAttrib {
  canAutoExecute: boolean;
  pastLimit: boolean;
  preOpen: boolean;
}

export interface TickAttribBidAsk {
  bidPastLow: boolean;
  askPastHigh: boolean;
}

export interface TickAttribLast {
  pastLimit: boolean;
  unreported: boolean;
}

export interface HistoricalTick {
  time: number;
  price: number;
  size: number;
}

export interface HistoricalTickBidAsk {
  time: number;
  tickAttribBidAsk: TickAttribBidAsk;
  priceBid: number;
  priceAsk: number;
  sizeBid: number;
  sizeAsk: number;
}

export interface HistoricalTickLast {
  time: number;
  tickAttribLast: TickAttribLast;
  price: number;
  size: number;
  exchange: string;
  specialConditions: string;
}

export interface HistoricalSession {
  startDateTime: string;
  endDateTime: string;
  refDate: string;
}

export interface HistoricalSchedule {
  startDateTime: string;
  endDateTime: string;
  timeZone: string;
  sessions: HistoricalSession[];
}

export interface ScannerSubscription {
  numberOfRows: number;
  instrument: string;
  locationCode: string;
  scanCode: string;
  abovePrice: number;
  belowPrice: number;
  aboveVolume: number;
  marketCapAbove: number;
  marketCapBelow: number;
  moodyRatingAbove: string;
  moodyRatingBelow: string;
  spRatingAbove: string;
  spRatingBelow: string;
  maturityDateAbove: string;
  maturityDateBelow: string;
  couponRateAbove: number;
  couponRateBelow: number;
  excludeConvertible: boolean;
  averageOptionVolumeAbove: number;
  scannerSettingPairs: string;
  stockTypeFilter: string;
}

export interface ScanData {
  rank: number;
  contractDetails: ContractDetails;
  distance: string;
  benchmark: string;
  projection: string;
  legsStr: string;
}

export interface WshEventData {
  conId: number;
  filter: string;
  fillWatchlist: boolean;
  fillPortfolio: boolean;
  fillCompetitors: boolean;
  startDate: string;
  endDate: string;
  totalLimit: number;
}

export interface FamilyCode {
  accountID: string;
  familyCodeStr: string;
}

export interface DepthMktDataDescription {
  exchange: string;
  secType: string;
  listingExch: string;
  serviceDataType: string;
  aggGroup: number;
}

export interface SmartComponent {
  bitNumber: number;
  exchange: string;
  exchangeLetter: string;
}

export interface NewsProvider {
  code: string;
  name: string;
}

export interface HistogramData {
  price: number;
  count: number;
}

export interface PriceIncrement {
  lowEdge: number;
  increment: number;
}

export interface AccountValue {
  account: string;
  tag: string;
  value: string;
  currency: string;
  modelCode: string;
}

export interface Position {
  account: string;
  contract: Contract;
  position: number;
  avgCost: number;
}

export interface PortfolioItem {
  contract: Contract;
  position: number;
  marketPrice: number;
  marketValue: number;
  averageCost: number;
  unrealizedPNL: number;
  realizedPNL: number;
  account: string;
}

export interface PnL {
  account: string;
  modelCode: string;
  dailyPnL: unknown;
  unrealizedPnL: unknown;
  realizedPnL: unknown;
}

export interface PnLSingle {
  account: string;
  modelCode: string;
  conId: number;
  dailyPnL: unknown;
  unrealizedPnL: unknown;
  realizedPnL: unknown;
  position: number;
  value: unknown;
}

export interface Fill {
  contract: Contract;
  execution: Execution;
  commissionReport: CommissionReport;
  time: number;
}

export interface TradeLogEntry {
  time: number;
  status: string;
  message: string;
  errorCode: number;
}

export interface OrderStatus {
  orderId: number;
  status: string;
  filled: number;
  remaining: number;
  avgFillPrice: number;
  permId: number;
  parentId: number;
  lastFillPrice: number;
  clientId: number;
  whyHeld: string;
  mktCapPrice: number;
}

export interface TickData {
  time: number;
  tickType: number;
  price: number;
  size: number;
}

export interface DOMLevel {
  price: number;
  size: number;
  marketMaker: string;
}

export interface MktDepthData {
  time: number;
  position: number;
  marketMaker: string;
  operation: number;
  side: number;
  price: number;
  size: number;
}

export interface Dividends {
  past12Months: unknown;
  next12Months: unknown;
  nextDate: unknown;
  nextAmount: unknown;
}

export interface OptionChain {
  exchange: string;
  underlyingConId: number;
  tradingClass: string;
  multiplier: string;
  expirations: unknown[];
  strikes: unknown[];
}

export interface NewsTick {
  timeStamp: number;
  providerCode: string;
  articleId: string;
  headline: string;
  extraData: string;
}

export interface NewsBulletin {
  msgId: number;
  msgType: number;
  message: string;
  origExchange: string;
}

export interface NewsArticle {
  articleType: number;
  articleText: string;
}

export interface HistoricalNews {
  time: string;
  providerCode: string;
  articleId: string;
  headline: string;
}

export interface OptionComputation {
  tickAttrib: unknown;
  impliedVol: unknown;
  delta: unknown;
  optPrice: unknown;
  pvDividend: unknown;
  gamma: unknown;
  vega: unknown;
  theta: unknown;
  undPrice: unknown;
}

export interface EfpData {
  basisPoints: number;
  formattedBasisPoints: string;
  impliedFuture: number;
  holdDays: number;
  futureLastTradeDate: string;
  dividendImpact: number;
  dividendsToLastTradeDate: number;
}

/** The Rust default of every generated type. */
export const DEFAULTS = {
  Contract: {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null} as Contract,
  ComboLeg: {"conId": 0, "ratio": 0, "action": "", "exchange": "", "openClose": 0, "shortSaleSlot": 0, "designatedLocation": "", "exemptCode": -1} as ComboLeg,
  DeltaNeutralContract: {"conId": 0, "delta": 0.0, "price": 0.0} as DeltaNeutralContract,
  ContractDetails: {"contract": {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null}, "marketName": "", "minTick": 0.0, "orderTypes": "", "validExchanges": "", "priceMagnifier": 0, "underConId": 0, "longName": "", "contractMonth": "", "industry": "", "category": "", "subcategory": "", "timeZoneId": "", "tradingHours": "", "liquidHours": "", "evRule": "", "evMultiplier": 0, "aggGroup": 0, "underSymbol": "", "underSecType": "", "marketRuleIds": "", "secIdList": [], "realExpirationDate": "", "lastTradeTime": "", "stockType": "", "minSize": 0.0, "sizeIncrement": 0.0, "suggestedSizeIncrement": 0.0, "cusip": "", "ratings": "", "descAppend": "", "bondType": "", "couponType": "", "callable": false, "putable": false, "coupon": 0.0, "convertible": false, "maturity": "", "issueDate": "", "nextOptionDate": "", "nextOptionType": "", "nextOptionPartial": false, "notes": "", "fundName": "", "fundFamily": "", "fundType": "", "fundFrontLoad": "", "fundBackLoad": "", "fundBackLoadTimeInterval": "", "fundManagementFee": "", "fundClosed": false, "fundClosedForNewInvestors": false, "fundClosedForNewMoney": false, "fundNotifyAmount": "", "fundMinimumInitialPurchase": "", "fundSubsequentMinimumPurchase": "", "fundBlueSkyStates": "", "fundBlueSkyTerritories": "", "fundDistributionPolicyIndicator": "", "fundAssetType": "", "ineligibilityReasonList": [], "eventContract1": "", "eventContractDescription1": "", "eventContractDescription2": "", "minAlgoSize": 0.0, "lastPricePrecision": 0.0, "lastSizePrecision": 0.0, "settlementMethod": ""} as ContractDetails,
  ContractDescription: {"contract": {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null}, "derivativeSecTypes": []} as ContractDescription,
  IneligibilityReason: {"id": "", "description": ""} as IneligibilityReason,
  Order: {"orderId": 0, "clientId": 0, "permId": 0, "action": "", "totalQuantity": 0.0, "orderType": "", "lmtPrice": UNSET_DOUBLE, "auxPrice": UNSET_DOUBLE, "tif": "", "activeStartTime": "", "activeStopTime": "", "ocaGroup": "", "ocaType": 0, "orderRef": "", "transmit": true, "parentId": 0, "blockOrder": false, "sweepToFill": false, "displaySize": 0, "triggerMethod": 0, "outsideRth": false, "hidden": false, "goodAfterTime": "", "goodTillDate": "", "rule80A": "", "allOrNone": false, "minQty": 2147483647, "percentOffset": UNSET_DOUBLE, "overridePercentageConstraints": false, "trailStopPrice": UNSET_DOUBLE, "trailingPercent": UNSET_DOUBLE, "faGroup": "", "faProfile": "", "faMethod": "", "faPercentage": "", "designatedLocation": "", "openClose": "O", "origin": 0, "shortSaleSlot": 0, "exemptCode": -1, "discretionaryAmt": 0.0, "eTradeOnly": false, "firmQuoteOnly": false, "nbboPriceCap": UNSET_DOUBLE, "optOutSmartRouting": false, "auctionStrategy": 0, "startingPrice": UNSET_DOUBLE, "stockRefPrice": UNSET_DOUBLE, "delta": UNSET_DOUBLE, "stockRangeLower": UNSET_DOUBLE, "stockRangeUpper": UNSET_DOUBLE, "randomizePrice": false, "randomizeSize": false, "volatility": UNSET_DOUBLE, "volatilityType": 2147483647, "deltaNeutralOrderType": "", "deltaNeutralAuxPrice": UNSET_DOUBLE, "deltaNeutralConId": 0, "deltaNeutralSettlingFirm": "", "deltaNeutralClearingAccount": "", "deltaNeutralClearingIntent": "", "deltaNeutralOpenClose": "", "deltaNeutralShortSale": false, "deltaNeutralShortSaleSlot": 0, "deltaNeutralDesignatedLocation": "", "continuousUpdate": false, "referencePriceType": 2147483647, "basisPoints": UNSET_DOUBLE, "basisPointsType": 2147483647, "scaleInitLevelSize": 2147483647, "scaleSubsLevelSize": 2147483647, "scalePriceIncrement": UNSET_DOUBLE, "scalePriceAdjustValue": UNSET_DOUBLE, "scalePriceAdjustInterval": 2147483647, "scaleProfitOffset": UNSET_DOUBLE, "scaleAutoReset": false, "scaleInitPosition": 2147483647, "scaleInitFillQty": 2147483647, "scaleRandomPercent": false, "scaleTable": "", "hedgeType": "", "hedgeParam": "", "hedgeMaxSize": 2147483647, "account": "", "settlingFirm": "", "clearingAccount": "", "clearingIntent": "", "algoStrategy": "", "algoParams": [], "smartComboRoutingParams": [], "algoId": "", "whatIf": false, "notHeld": false, "solicited": false, "modelCode": "", "orderComboLegs": [], "orderMiscOptions": [], "referenceContractId": 0, "peggedChangeAmount": 0.0, "isPeggedChangeAmountDecrease": false, "referenceChangeAmount": 0.0, "referenceExchangeId": "", "adjustedOrderType": "", "triggerPrice": UNSET_DOUBLE, "adjustedStopPrice": UNSET_DOUBLE, "adjustedStopLimitPrice": UNSET_DOUBLE, "adjustedTrailingAmount": UNSET_DOUBLE, "adjustableTrailingUnit": 0, "lmtPriceOffset": UNSET_DOUBLE, "conditions": [], "conditionsCancelOrder": false, "conditionsIgnoreRth": false, "conditionsIncludeOvernight": false, "extOperator": "", "softDollarTier": {"name": "", "val": "", "displayName": ""}, "cashQty": UNSET_DOUBLE, "mifid2DecisionMaker": "", "mifid2DecisionAlgo": "", "mifid2ExecutionTrader": "", "mifid2ExecutionAlgo": "", "dontUseAutoPriceForHedge": false, "isOmsContainer": false, "discretionaryUpToLimitPrice": false, "autoCancelDate": "", "filledQuantity": UNSET_DOUBLE, "refFuturesConId": 0, "autoCancelParent": false, "shareholder": "", "imbalanceOnly": false, "routeMarketableToBbo": null, "parentPermId": 0, "usePriceMgmtAlgo": null, "duration": 2147483647, "postToAts": 2147483647, "advancedErrorOverride": "", "manualOrderTime": "", "minTradeQty": 2147483647, "minCompeteSize": 2147483647, "competeAgainstBestOffset": UNSET_DOUBLE, "midOffsetAtWhole": UNSET_DOUBLE, "midOffsetAtHalf": UNSET_DOUBLE, "customerAccount": "", "professionalCustomer": false, "bondAccruedInterest": "", "includeOvernight": false, "manualOrderIndicator": 2147483647, "submitter": "", "postOnly": false, "allowPreOpen": false, "ignoreOpenAuction": false, "deactivate": false, "seekPriceImprovement": null, "whatIfType": 2147483647, "slOrderId": 2147483647, "slOrderType": "", "ptOrderId": 2147483647, "ptOrderType": ""} as Order,
  OrderState: {"status": "", "initMarginBefore": "", "maintMarginBefore": "", "equityWithLoanBefore": "", "initMarginChange": "", "maintMarginChange": "", "equityWithLoanChange": "", "initMarginAfter": "", "maintMarginAfter": "", "equityWithLoanAfter": "", "commission": UNSET_DOUBLE, "minCommission": UNSET_DOUBLE, "maxCommission": UNSET_DOUBLE, "commissionCurrency": "", "marginCurrency": "", "initMarginBeforeOutsideRth": UNSET_DOUBLE, "maintMarginBeforeOutsideRth": UNSET_DOUBLE, "equityWithLoanBeforeOutsideRth": UNSET_DOUBLE, "initMarginChangeOutsideRth": UNSET_DOUBLE, "maintMarginChangeOutsideRth": UNSET_DOUBLE, "equityWithLoanChangeOutsideRth": UNSET_DOUBLE, "initMarginAfterOutsideRth": UNSET_DOUBLE, "maintMarginAfterOutsideRth": UNSET_DOUBLE, "equityWithLoanAfterOutsideRth": UNSET_DOUBLE, "suggestedSize": UNSET_DOUBLE, "rejectReason": "", "orderAllocations": [], "warningText": "", "completedTime": "", "completedStatus": ""} as OrderState,
  OrderAllocation: {"account": "", "position": 0.0, "positionDesired": 0.0, "positionAfter": 0.0, "desiredAllocQty": 0.0, "allowedAllocQty": 0.0, "isMonetary": false} as OrderAllocation,
  OrderComboLeg: {"price": UNSET_DOUBLE} as OrderComboLeg,
  OrderCancel: {"manualOrderCancelTime": "", "extOperator": "", "manualOrderIndicator": 2147483647} as OrderCancel,
  SoftDollarTier: {"name": "", "val": "", "displayName": ""} as SoftDollarTier,
  Execution: {"execId": "", "time": "", "acctNumber": "", "exchange": "", "side": "", "shares": 0.0, "price": 0.0, "permId": 0, "clientId": 0, "orderId": 0, "liquidation": 0, "cumQty": 0.0, "avgPrice": 0.0, "orderRef": "", "evRule": "", "evMultiplier": 0.0, "modelCode": "", "lastLiquidity": 0, "pendingPriceRevision": false, "submitter": "", "optExerciseOrLapseType": 0} as Execution,
  ExecutionFilter: {"clientId": 0, "acctCode": "", "time": "", "symbol": "", "secType": "", "exchange": "", "side": "", "lastNDays": 2147483647, "specificDates": []} as ExecutionFilter,
  CommissionReport: {"execId": "", "commission": 0.0, "currency": "", "realizedPNL": 0.0, "yield_": 0.0, "yieldRedemptionDate": 0} as CommissionReport,
  BarData: {"date": "", "open": 0.0, "high": 0.0, "low": 0.0, "close": 0.0, "volume": 0.0, "average": 0.0, "barCount": 0} as BarData,
  RealTimeBar: {"time": 0, "open_": 0.0, "high": 0.0, "low": 0.0, "close": 0.0, "volume": 0.0, "wap": 0.0, "count": 0} as RealTimeBar,
  TickAttrib: {"canAutoExecute": false, "pastLimit": false, "preOpen": false} as TickAttrib,
  TickAttribBidAsk: {"bidPastLow": false, "askPastHigh": false} as TickAttribBidAsk,
  TickAttribLast: {"pastLimit": false, "unreported": false} as TickAttribLast,
  HistoricalTick: {"time": 0, "price": 0.0, "size": 0.0} as HistoricalTick,
  HistoricalTickBidAsk: {"time": 0, "tickAttribBidAsk": {"bidPastLow": false, "askPastHigh": false}, "priceBid": 0.0, "priceAsk": 0.0, "sizeBid": 0.0, "sizeAsk": 0.0} as HistoricalTickBidAsk,
  HistoricalTickLast: {"time": 0, "tickAttribLast": {"pastLimit": false, "unreported": false}, "price": 0.0, "size": 0.0, "exchange": "", "specialConditions": ""} as HistoricalTickLast,
  HistoricalSession: {"startDateTime": "", "endDateTime": "", "refDate": ""} as HistoricalSession,
  HistoricalSchedule: {"startDateTime": "", "endDateTime": "", "timeZone": "", "sessions": []} as HistoricalSchedule,
  ScannerSubscription: {"numberOfRows": -1, "instrument": "", "locationCode": "", "scanCode": "", "abovePrice": UNSET_DOUBLE, "belowPrice": UNSET_DOUBLE, "aboveVolume": 2147483647, "marketCapAbove": UNSET_DOUBLE, "marketCapBelow": UNSET_DOUBLE, "moodyRatingAbove": "", "moodyRatingBelow": "", "spRatingAbove": "", "spRatingBelow": "", "maturityDateAbove": "", "maturityDateBelow": "", "couponRateAbove": UNSET_DOUBLE, "couponRateBelow": UNSET_DOUBLE, "excludeConvertible": false, "averageOptionVolumeAbove": 2147483647, "scannerSettingPairs": "", "stockTypeFilter": ""} as ScannerSubscription,
  ScanData: {"rank": 0, "contractDetails": {"contract": {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null}, "marketName": "", "minTick": 0.0, "orderTypes": "", "validExchanges": "", "priceMagnifier": 0, "underConId": 0, "longName": "", "contractMonth": "", "industry": "", "category": "", "subcategory": "", "timeZoneId": "", "tradingHours": "", "liquidHours": "", "evRule": "", "evMultiplier": 0, "aggGroup": 0, "underSymbol": "", "underSecType": "", "marketRuleIds": "", "secIdList": [], "realExpirationDate": "", "lastTradeTime": "", "stockType": "", "minSize": 0.0, "sizeIncrement": 0.0, "suggestedSizeIncrement": 0.0, "cusip": "", "ratings": "", "descAppend": "", "bondType": "", "couponType": "", "callable": false, "putable": false, "coupon": 0.0, "convertible": false, "maturity": "", "issueDate": "", "nextOptionDate": "", "nextOptionType": "", "nextOptionPartial": false, "notes": "", "fundName": "", "fundFamily": "", "fundType": "", "fundFrontLoad": "", "fundBackLoad": "", "fundBackLoadTimeInterval": "", "fundManagementFee": "", "fundClosed": false, "fundClosedForNewInvestors": false, "fundClosedForNewMoney": false, "fundNotifyAmount": "", "fundMinimumInitialPurchase": "", "fundSubsequentMinimumPurchase": "", "fundBlueSkyStates": "", "fundBlueSkyTerritories": "", "fundDistributionPolicyIndicator": "", "fundAssetType": "", "ineligibilityReasonList": [], "eventContract1": "", "eventContractDescription1": "", "eventContractDescription2": "", "minAlgoSize": 0.0, "lastPricePrecision": 0.0, "lastSizePrecision": 0.0, "settlementMethod": ""}, "distance": "", "benchmark": "", "projection": "", "legsStr": ""} as ScanData,
  WshEventData: {"conId": 2147483647, "filter": "", "fillWatchlist": false, "fillPortfolio": false, "fillCompetitors": false, "startDate": "", "endDate": "", "totalLimit": 2147483647} as WshEventData,
  FamilyCode: {"accountID": "", "familyCodeStr": ""} as FamilyCode,
  DepthMktDataDescription: {"exchange": "", "secType": "", "listingExch": "", "serviceDataType": "", "aggGroup": 2147483647} as DepthMktDataDescription,
  SmartComponent: {"bitNumber": 0, "exchange": "", "exchangeLetter": ""} as SmartComponent,
  NewsProvider: {"code": "", "name": ""} as NewsProvider,
  HistogramData: {"price": 0.0, "count": 0.0} as HistogramData,
  PriceIncrement: {"lowEdge": 0.0, "increment": 0.0} as PriceIncrement,
  AccountValue: {"account": "", "tag": "", "value": "", "currency": "", "modelCode": ""} as AccountValue,
  Position: {"account": "", "contract": {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null}, "position": 0.0, "avgCost": 0.0} as Position,
  PortfolioItem: {"contract": {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null}, "position": 0.0, "marketPrice": 0.0, "marketValue": 0.0, "averageCost": 0.0, "unrealizedPNL": 0.0, "realizedPNL": 0.0, "account": ""} as PortfolioItem,
  PnL: {"account": "", "modelCode": "", "dailyPnL": null, "unrealizedPnL": null, "realizedPnL": null} as PnL,
  PnLSingle: {"account": "", "modelCode": "", "conId": 0, "dailyPnL": null, "unrealizedPnL": null, "realizedPnL": null, "position": 0.0, "value": null} as PnLSingle,
  Fill: {"contract": {"conId": 0, "symbol": "", "secType": "", "lastTradeDateOrContractMonth": "", "lastTradeDate": "", "strike": 0.0, "right": "", "multiplier": "", "exchange": "", "primaryExchange": "", "currency": "", "localSymbol": "", "tradingClass": "", "includeExpired": false, "secIdType": "", "secId": "", "description": "", "issuerId": "", "comboLegsDescrip": "", "comboLegs": [], "deltaNeutralContract": null}, "execution": {"execId": "", "time": "", "acctNumber": "", "exchange": "", "side": "", "shares": 0.0, "price": 0.0, "permId": 0, "clientId": 0, "orderId": 0, "liquidation": 0, "cumQty": 0.0, "avgPrice": 0.0, "orderRef": "", "evRule": "", "evMultiplier": 0.0, "modelCode": "", "lastLiquidity": 0, "pendingPriceRevision": false, "submitter": "", "optExerciseOrLapseType": 0}, "commissionReport": {"execId": "", "commission": 0.0, "currency": "", "realizedPNL": 0.0, "yield_": 0.0, "yieldRedemptionDate": 0}, "time": 0.0} as Fill,
  TradeLogEntry: {"time": 0.0, "status": "", "message": "", "errorCode": 0} as TradeLogEntry,
  OrderStatus: {"orderId": 0, "status": "", "filled": 0.0, "remaining": 0.0, "avgFillPrice": 0.0, "permId": 0, "parentId": 0, "lastFillPrice": 0.0, "clientId": 0, "whyHeld": "", "mktCapPrice": 0.0} as OrderStatus,
  TickData: {"time": 0.0, "tickType": 0, "price": 0.0, "size": 0.0} as TickData,
  DOMLevel: {"price": 0.0, "size": 0.0, "marketMaker": ""} as DOMLevel,
  MktDepthData: {"time": 0.0, "position": 0, "marketMaker": "", "operation": 0, "side": 0, "price": 0.0, "size": 0.0} as MktDepthData,
  Dividends: {"past12Months": null, "next12Months": null, "nextDate": null, "nextAmount": null} as Dividends,
  OptionChain: {"exchange": "", "underlyingConId": 0, "tradingClass": "", "multiplier": "", "expirations": [], "strikes": []} as OptionChain,
  NewsTick: {"timeStamp": 0, "providerCode": "", "articleId": "", "headline": "", "extraData": ""} as NewsTick,
  NewsBulletin: {"msgId": 0, "msgType": 0, "message": "", "origExchange": ""} as NewsBulletin,
  NewsArticle: {"articleType": 0, "articleText": ""} as NewsArticle,
  HistoricalNews: {"time": "", "providerCode": "", "articleId": "", "headline": ""} as HistoricalNews,
  OptionComputation: {"tickAttrib": null, "impliedVol": null, "delta": null, "optPrice": null, "pvDividend": null, "gamma": null, "vega": null, "theta": null, "undPrice": null} as OptionComputation,
  EfpData: {"basisPoints": 0.0, "formattedBasisPoints": "", "impliedFuture": 0.0, "holdDays": 0, "futureLastTradeDate": "", "dividendImpact": 0.0, "dividendsToLastTradeDate": 0.0} as EfpData,
} as const;
