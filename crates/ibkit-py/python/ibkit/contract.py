"""Contracts (ib_async ``contract.py``)."""

from __future__ import annotations

from typing import Any, cast

from . import _types, util
from ._types import (
    ComboLeg,
    ContractDescription,
    ContractDetails,
    DeltaNeutralContract,
    TagValue,
)

__all__ = [
    'CFD',
    'Bag',
    'Bond',
    'ComboLeg',
    'Commodity',
    'ContFuture',
    'Contract',
    'ContractDescription',
    'ContractDetails',
    'Crypto',
    'DeltaNeutralContract',
    'Forex',
    'Future',
    'FuturesOption',
    'Index',
    'MutualFund',
    'Option',
    'Stock',
    'TagValue',
    'Warrant',
]


class Contract(_types.Contract):
    """A financial instrument. Use the subclasses (``Stock``, ``Option``, ...) or
    ``Contract.create(secType=..., ...)``."""

    @staticmethod
    def create(**kwargs: Any) -> Contract:
        cls = _BY_SEC_TYPE.get(kwargs.get('secType', ''), Contract)
        obj = _types.Contract(**kwargs)
        obj.__class__ = cls
        return cast(Contract, obj)

    @staticmethod
    def recreate(c: _types.Contract) -> Contract:
        return Contract.create(**util.dataclassAsDict(c))

    def isHashable(self) -> bool:
        return self.conId != 0 or self.secType == 'CONTFUT'

    def pair(self) -> str:
        return self.symbol + self.currency if self.secType == 'CASH' else ''

    def __hash__(self) -> int:
        if self.conId:
            return hash(
                ('CONTFUT', self.conId) if self.secType == 'CONTFUT' else self.conId
            )
        return hash(
            (
                self.secType,
                self.symbol,
                self.lastTradeDateOrContractMonth,
                self.strike,
                self.right,
                self.exchange,
                self.currency,
                self.localSymbol,
            )
        )


class Stock(Contract):
    def __init__(
        self, symbol: str = '', exchange: str = '', currency: str = '', **kwargs: Any
    ) -> None:
        _types.Contract.__init__(
            self,
            secType='STK',
            symbol=symbol,
            exchange=exchange,
            currency=currency,
            **kwargs,
        )


class Option(Contract):
    def __init__(
        self,
        symbol: str = '',
        lastTradeDateOrContractMonth: str = '',
        strike: float = 0.0,
        right: str = '',
        exchange: str = '',
        multiplier: str = '',
        currency: str = '',
        **kwargs: Any,
    ) -> None:
        _types.Contract.__init__(
            self,
            secType=kwargs.pop('secType', 'OPT'),
            symbol=symbol,
            lastTradeDateOrContractMonth=lastTradeDateOrContractMonth,
            strike=strike,
            right=right,
            exchange=exchange,
            multiplier=multiplier,
            currency=currency,
            **kwargs,
        )


class Future(Contract):
    def __init__(
        self,
        symbol: str = '',
        lastTradeDateOrContractMonth: str = '',
        exchange: str = '',
        localSymbol: str = '',
        multiplier: str = '',
        currency: str = '',
        **kwargs: Any,
    ) -> None:
        _types.Contract.__init__(
            self,
            secType='FUT',
            symbol=symbol,
            lastTradeDateOrContractMonth=lastTradeDateOrContractMonth,
            exchange=exchange,
            localSymbol=localSymbol,
            multiplier=multiplier,
            currency=currency,
            **kwargs,
        )


class ContFuture(Contract):
    def __init__(
        self,
        symbol: str = '',
        exchange: str = '',
        localSymbol: str = '',
        multiplier: str = '',
        currency: str = '',
        **kwargs: Any,
    ) -> None:
        _types.Contract.__init__(
            self,
            secType='CONTFUT',
            symbol=symbol,
            exchange=exchange,
            localSymbol=localSymbol,
            multiplier=multiplier,
            currency=currency,
            **kwargs,
        )


class Forex(Contract):
    def __init__(
        self,
        pair: str = '',
        exchange: str = 'IDEALPRO',
        symbol: str = '',
        currency: str = '',
        **kwargs: Any,
    ) -> None:
        if pair:
            if len(pair) != 6:
                raise ValueError(
                    f'Forex pair {pair!r} must have 6 letters, for example "EURUSD"'
                )
            symbol, currency = symbol or pair[:3], currency or pair[3:]
        _types.Contract.__init__(
            self,
            secType='CASH',
            symbol=symbol,
            exchange=exchange,
            currency=currency,
            **kwargs,
        )


class _SymbolExchangeCurrency(Contract):
    _secType = ''

    def __init__(
        self, symbol: str = '', exchange: str = '', currency: str = '', **kwargs: Any
    ) -> None:
        _types.Contract.__init__(
            self,
            secType=self._secType,
            symbol=symbol,
            exchange=exchange,
            currency=currency,
            **kwargs,
        )


class Index(_SymbolExchangeCurrency):
    _secType = 'IND'


class CFD(_SymbolExchangeCurrency):
    _secType = 'CFD'


class Commodity(_SymbolExchangeCurrency):
    _secType = 'CMDTY'


class Crypto(_SymbolExchangeCurrency):
    _secType = 'CRYPTO'


class Bond(Contract):
    def __init__(self, **kwargs: Any) -> None:
        _types.Contract.__init__(self, secType='BOND', **kwargs)


class FuturesOption(Option):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        Option.__init__(self, *args, secType='FOP', **kwargs)


class MutualFund(Contract):
    def __init__(self, **kwargs: Any) -> None:
        _types.Contract.__init__(self, secType='FUND', **kwargs)


class Warrant(Contract):
    def __init__(self, **kwargs: Any) -> None:
        _types.Contract.__init__(self, secType='WAR', **kwargs)


class Bag(Contract):
    def __init__(self, **kwargs: Any) -> None:
        _types.Contract.__init__(self, secType='BAG', **kwargs)


_BY_SEC_TYPE: dict[str, type[Contract]] = {
    'STK': Stock,
    'OPT': Option,
    'FUT': Future,
    'CONTFUT': ContFuture,
    'CASH': Forex,
    'IND': Index,
    'CFD': CFD,
    'BOND': Bond,
    'CMDTY': Commodity,
    'FOP': FuturesOption,
    'FUND': MutualFund,
    'WAR': Warrant,
    'IOPT': Warrant,
    'BAG': Bag,
    'CRYPTO': Crypto,
}

util.CLASSES['Contract'] = Contract
util.FACTORIES['Contract'] = lambda values: _BY_SEC_TYPE.get(
    values.get('secType', ''), Contract
)
