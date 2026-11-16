"""Download Flex queries from the IBKR Flex Web Service (ib_async ``FlexReport``)."""

from __future__ import annotations

import asyncio
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as et
from typing import Any

from .errors import IbkitError

FLEX_URL = 'https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService'


class FlexError(IbkitError):
    """The Flex Web Service refused a request or the statement was not ready."""


class FlexReport:
    """Download a Flex query with ``FlexReport(token, queryId)`` or load a
    saved one with ``FlexReport(path=...)``. ``extract(topic)`` returns the
    rows of one topic (for example ``'Trade'``) as dictionaries."""

    def __init__(
        self,
        token: str | None = None,
        queryId: str | None = None,
        path: str | None = None,
    ) -> None:
        self.data: bytes = b''
        self.root: et.Element | None = None
        if token and queryId:
            self.download(token, queryId)
        elif path:
            self.load(path)

    def topics(self) -> set[str]:
        return {node.tag for node in self._root().iter() if node.attrib}

    def extract(self, topic: str) -> list[dict[str, Any]]:
        return [dict(node.attrib) for node in self._root().iter(topic) if node.attrib]

    def df(self, topic: str) -> Any:
        import pandas as pd  # pyright: ignore[reportMissingImports]

        return pd.DataFrame(self.extract(topic))

    def _root(self) -> et.Element:
        if self.root is None:
            raise FlexError('no report loaded; call download() or load() first')
        return self.root

    def download(
        self, token: str, queryId: str, retries: int = 10, wait: float = 5.0
    ) -> None:
        query = urllib.parse.urlencode({'t': token, 'q': queryId, 'v': '3'})
        with urllib.request.urlopen(
            f'{FLEX_URL}/SendRequest?{query}', timeout=60
        ) as resp:
            root = et.fromstring(resp.read())
        if root.findtext('Status') != 'Success':
            raise FlexError(
                f'Flex request failed: {root.findtext("ErrorCode")} {root.findtext("ErrorMessage")}; check the token and query id in Client Portal > Reports > Flex Queries'
            )
        code, url = root.findtext('ReferenceCode'), root.findtext('Url')
        for _ in range(retries):
            fetch = urllib.parse.urlencode({'q': code, 't': token, 'v': '3'})
            with urllib.request.urlopen(f'{url}?{fetch}', timeout=60) as resp:
                data = resp.read()
            node = et.fromstring(data)
            if node.tag != 'FlexStatementResponse' or node.findtext('Status') != 'Warn':
                self.data, self.root = data, node
                return
            time.sleep(wait)
        raise FlexError(
            f'the Flex statement was not ready after {retries} tries; try again later'
        )

    async def downloadAsync(
        self, token: str, queryId: str, retries: int = 10, wait: float = 5.0
    ) -> None:
        """``download`` for asyncio; the HTTP requests run in a thread."""
        await asyncio.to_thread(self.download, token, queryId, retries, wait)

    def load(self, path: str) -> None:
        with open(path, 'rb') as f:
            self.data = f.read()
        self.root = et.fromstring(self.data)

    def save(self, path: str) -> None:
        with open(path, 'wb') as f:
            f.write(self.data)
