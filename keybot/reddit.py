#!/usr/bin/env python3

import sys, os
import http.server
import concurrent.futures
import webbrowser
import urllib.parse
import uuid
import logging
import datetime
import typing

import requests
import requests.auth

def getLogger(name=__name__, level=logging.DEBUG):
    logger = logging.getLogger(name)
    logger.setLevel(level)
    handler = logging.StreamHandler(sys.stderr)
    formatter = logging.Formatter("%(levelname)s - %(message)s")
    handler.setFormatter(formatter)
    logger.addHandler(handler)
    return logger

Logger = getLogger()

class AuthCallbackHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.Params = urllib.parse.parse_qs(
            urllib.parse.urlparse(self.path).query)
        self.send_response(200)
        self.end_headers()
        self.wfile.write("It's a trap!".encode("utf-8"))
        self.wfile.flush()
        self.close_connection = True

    def log_message(self, format, *args, **kargs):
        pass

class RedditQuerier(object):
    UrlBase = "https://oauth.reddit.com"
    UserAgent = "desktop:org.darksair.keybot:0.0.1 (by /u/darksair)"

    def __init__(self):
        self.Token = None
        self.RefreshToken = None
        self.HttpSession = requests.Session()
        self.HttpSession.headers["User-Agent"] = self.UserAgent

    def authenticateUserLess(self, client_id, client_secret):
        Url = "https://www.reddit.com/api/v1/access_token"
        Res = self.HttpSession.post(
            Url, data={"grant_type": "client_credentials"},
            auth=requests.auth.HTTPBasicAuth(client_id, client_secret))
        try:
            Res.raise_for_status()
        except requests.HTTPError as Err:
            raise RuntimeError("Failed to get token: " + str(Err))

        Data = Res.json()
        self.Token = Data["access_token"]
        self.HttpSession.headers["Authorization"] =  "bearer " + self.Token

    def authenticate(self, client_id, client_secret):
        StateStr = uuid.uuid1().hex
        InitPayload = {"client_id": client_id,
                       "response_type": "code",
                       "state": StateStr,
                       "redirect_uri": "http://localhost:31416/",
                       "duration": "temporary",
                       "scope": "identity edit flair history mysubreddits "
                       "privatemessages read report save submit subscribe vote "
                       "wikiedit wikiread"}

        InitUrl = "https://www.reddit.com/api/v1/authorize"

        PermUrl = InitUrl + '?' + urllib.parse.urlencode(InitPayload)
        webbrowser.open(PermUrl)
        CallbackServer = http.server.HTTPServer(("", 31416), AuthCallbackHandler)
        try:
            CallbackServer.handle_request()
        except KeyboardInterrupt:
            CallbackServer.socket.close()

        if hasattr(CallbackServer, "Params"):
            if CallbackServer.Params["state"][0] != StateStr:
                raise RuntimeError("Failed to get token. State doesn't match.")
            if "error" in CallbackServer.Params:
                raise RuntimeError("Failed to get code: " +
                                   CallbackServer.Params["error"][0])

            Code = CallbackServer.Params["code"][0]

        TokenPayload = {"grant_type": "authorization_code",
                        "code": Code,
                        "redirect_uri": "http://localhost:31416/"}

        Res = self.HttpSession.post(
            "https://www.reddit.com/api/v1/access_token", data=TokenPayload,
            auth=requests.auth.HTTPBasicAuth(client_id, client_secret))
        try:
            Res.raise_for_status()
        except requests.HTTPError as Err:
            raise RuntimeError("Failed to get token: " + str(Err))
        Data = Res.json()
        self.Token = Data["access_token"]
        self.RefreshToken = Data.get("refresh_token")
        self.HttpSession.headers.update({"Authorization": "bearer " + self.Token,
                                         "User-Agent": self.UserAgent})

    def logout(self, client_id, client_secret):
        Payload = {"token": self.Token, "token_type_hint": "access_token"}
        Res = requests.post("https://www.reddit.com/api/v1/revoke_token",
                            data=Payload, headers={"User-Agent": self.UserAgent},
                            auth=requests.auth.HTTPBasicAuth(client_id, client_secret))
        try:
            Res.raise_for_status()
        except requests.HTTPError as Err:
            raise RuntimeError("Failed to logout: " + str(Err))

    def _urlPreprocess(self, url):
        Component = urllib.parse.urlparse(url)
        if Component.query == "":
            Url = url + "?raw_json=1"
        else:
            Url = url + "&raw_json=1"

        if not url.startswith(self.UrlBase):
            Url = self.UrlBase + Url

        return Url

    def get(self, url, *args, **kargs):
        Res = self.HttpSession.get(self._urlPreprocess(url), *args, **kargs)
        try:
            Res.raise_for_status()
        except requests.HTTPError as Err:
            raise RuntimeError("Failed to query {}: {}".format(url, Err))
        else:
            return Res

    def post(self, url, *args, **kargs):
        Res = self.HttpSession.post(self._urlPreprocess(Url), *args, **kargs)
        try:
            Res.raise_for_status()
        except requests.HTTPError as Err:
            raise RuntimeError("Failed to query {}: {}".format(url, Err))
        else:
            return Res

RedditQuery = RedditQuerier()

class RedditPost(object):
    def __init__(self):
        self.Title = ""
        self.Text = ""
        self.Author = ""
        self.Score = 0
        self.Url = ""           # URI to the post itself
        self.Link = ""          # Link in the post
        self.HideScore = False
        self.ID = ""            # “Fullname”
        self.CountComments = 0
        self.TimeCreate = 0     # type: datetime.datetime
        self.Sub = ""

    @property
    def FullName(self) -> str:
        return self.ID

    @property
    def IsLink(self) -> bool:
        if self.Link.startswith(
                "https://www.reddit.com/r/{}/comments".format(self.Sub)):
            if self.Link[len("https://www.reddit.com"):] == self.Url:
                return False
        return True

    @property
    def ShortUrl(self) -> str:
        UID = self.ID.split('_', 1)[1]
        return "https://reddit.com/r/{}/comments/{}/".format(self.Sub, UID)

class PostSorting(object):
    Hot = "hot"
    New = "new"

class SubredditInfo(object):
    def __init__(self, name):
        self.Name = name

    @property
    def UrlName(self):
        return "/r/" + self.Name

    def list(self, sorting: str, before: str=None, after: str=None, limit=50) \
        -> typing.List[RedditPost]:
        Query = {"g": "GLOBAL"}
        if before is not None:
            Query["before"] = before
        elif after is not None:
            Query["after"] = after

        Res = RedditQuery.get("{}/{}.json".format(self.UrlName, sorting),
                              params=Query)
        Data = Res.json()
        Posts = []
        for Item in Data["data"]["children"]:
            Data = Item["data"]
            Post = RedditPost()
            Post.Title = Data["title"]
            Post.Text = Data["selftext"]
            Post.Author = Data["author"]
            Post.Score = Data["score"]
            Post.Url = Data["permalink"]
            Post.Link = Data["url"]
            Post.HideScore = Data["hide_score"]
            Post.ID = Data["name"]
            Post.CountComments = Data["num_comments"]
            Post.TimeCreate = datetime.datetime.fromtimestamp(Data["created_utc"])
            Post.Sub = Data["subreddit"]
            Posts.append(Post)

        return Posts
