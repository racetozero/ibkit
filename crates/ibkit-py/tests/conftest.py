"""Shared fixtures: a fake TWS server (the Rust one) with scripted answers.

The answers are TWS messages at server version 178. Request fields are
indexed as the encoder writes them (message id first).
"""

from __future__ import annotations

import pytest
from ibkit import _ibkit

MAX = '1.7976931348623157e308'
ACCOUNT = 'DU123456'


def m(*fields: object) -> list[str]:
    return [str(f) for f in fields]


# fmt: off
def open_order(order_id: object, perm_id: int, status: str, symbol: str = 'AAPL', what_if: bool = False, margin: str = '') -> list[str]:
    f = m(5, order_id, 265598, symbol, 'STK', '', 0, '', '', 'SMART', 'USD', symbol, 'NMS', 'BUY', 100, 'LMT', 150, '', 'DAY', '', ACCOUNT, 'O', 0, '', 1, perm_id, 0, 0, 0, '', '')
    f += m('', '', '', '', '', '', '', '', 0, '', -1)
    f += m(0, '', '', '', '', '', 0, 0, 0, 0, '', 0, 0, 0, '', 0, 0)
    f += m('', 0, '', '', 0, 0, '', '', '', '', '', 0, 0, 0, '', '', '', '', 0, '', '', 0, 0, '')
    f += m(0, int(what_if), status, '', '', '', margin, '', '', '', '', '', 1.5, '', '', 'USD', '')
    f += m(0, 0, 0, '', MAX, MAX, MAX, MAX, MAX, MAX, 0, '', '', '', MAX, 0, 0, 0, 0, '', '', 0, '', '', '', '', '')
    return f


def order_status(order_id: object, status: str, filled: float, remaining: float, avg: float) -> list[str]:
    return m(3, order_id, status, filled, remaining, avg, 777, 0, avg, 1, '', 0)


CONTRACT_AAPL = ['AAPL', 'STK', '', 0, '', 'SMART', 'USD', 'AAPL', 'NMS', 'NMS', 265598, 0.01, 1, 'LMT', 'SMART,NASDAQ', 1, 0, 'APPLE INC', 'NASDAQ', '', 'Technology', '', '', 'US/Eastern', '', '', '', '', 0, 1, 'AAPL', '', '26', '', 'COMMON', 0.0001, 0.0001, 100]
BARS = [2, '20260102', 1, 2, 0.5, 1.5, 100, 1.2, 10, '20260105', 1.5, 2.5, 1, 2, 200, 1.8, 20]


def place_order(req: list[str]) -> list[list[str]]:
    oid, symbol = req[1], req[3]
    if symbol == 'MSFT':
        return [open_order(oid, 778, 'Submitted', 'MSFT'), order_status(oid, 'Submitted', 0, 100, 0)]
    if symbol == 'REJ':
        return [m(4, 2, oid, 201, 'Order rejected', '')]
    if symbol == 'WHATIF':
        return [open_order(oid, 779, 'PreSubmitted', 'WHATIF', True, '2500.5')]
    return [
        open_order(oid, 777, 'Submitted'),
        order_status(oid, 'Submitted', 0, 100, 0),
        m(11, -1, oid, 265598, 'AAPL', 'STK', '', 0, '', '', 'ISLAND', 'USD', 'AAPL', 'NMS', 'e1', '20260102 10:00:00', ACCOUNT, 'ISLAND', 'BOT', 100, 150, 777, 1, 0, 100, 150, '', '', '', '', 1, 0),
        order_status(oid, 'Filled', 100, 0, 150),
        m(59, 1, 'e1', 1.0, 'USD', MAX, MAX, ''),
    ]


def responder(req: list[str]) -> list[list[str]]:
    msg_id = int(req[0])
    if msg_id == 61:
        return [m(61, 3, ACCOUNT, 8314, 'IBM', 'STK', '', 0, '', '', 'NYSE', 'USD', 'IBM', 'IBM', 10, 120.5), m(62, 1)]
    if msg_id == 5:
        return [m(53, 1)]
    if msg_id == 99:
        return [m(102)]
    if msg_id == 6:
        return [m(6, 2, 'NetLiquidation', '100000', 'USD', ACCOUNT), m(54, 1, ACCOUNT)]
    if msg_id == 76:
        return [m(74, 1, req[2])]
    if msg_id == 7:
        return [m(55, 1, req[2])]
    if msg_id == 9:
        if req[4] == 'XYZ':
            return [m(4, 2, req[2], 200, 'No security definition has been found for the request', '')]
        return [m(10, req[2], *CONTRACT_AAPL), m(52, 1, req[2])]
    if msg_id == 3:
        return place_order(req)
    if msg_id == 4:  # CANCEL_ORDER at 178: [4, 1, orderId, manualCancelTime]
        return [order_status(req[2], 'Cancelled', 0, 100, 0)]
    if msg_id == 20:  # keepUpToDate is the second-to-last field
        out = [m(17, req[1], 'a', 'b', *BARS)]
        if req[-2] == '1':
            out.append(m(90, req[1], 1, '20260106', 2.2, 2.3, 2.4, 2.1, 2.25, 10))
        return out
    if msg_id == 10:
        r = req[2]
        return [m(13, 1, r, 0, 'NSDQ', 0, 1, 150.25, 300, 0), m(13, 1, r, 0, 'NSDQ', 0, 0, 150.5, 200, 0)]
    if msg_id == 1:  # snapshot flag at index 17
        r = req[2]
        out = [m(1, 6, r, 1, 150.25, 300, 1), m(1, 6, r, 2, 150.5, 200, 1), m(1, 6, r, 4, 150.4, 5, 0), m(2, 6, r, 8, 12345)]
        if req[17] == '1':
            out.append(m(57, 1, r))
        return out
    if msg_id == 49:
        return [m(49, 1, 1767225600)]
    if msg_id == 100:
        return [m(104, req[1], '{"meta":1}')]
    if msg_id == 102:
        return [m(105, req[1], '{"event":' + req[2] + '}')]
    return []
# fmt: on


@pytest.fixture
def tws():
    server = _ibkit.FakeTws(178, 1000, ACCOUNT, responder)
    yield server
    server.close()
