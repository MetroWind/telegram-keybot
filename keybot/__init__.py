#!/usr/bin/env python

import sys, os
import typing
import logging
import configparser
import string
import datetime
import json
import fcntl
from html import escape

import telegram
import telegram.ext as BotAPI
from telegram.ext.dispatcher import run_async # Not used

from . import reddit

def getLogger(name=__name__, level=logging.INFO):
    logger = logging.getLogger(name)
    logger.setLevel(level)
    handler = logging.StreamHandler(sys.stderr)
    formatter = logging.Formatter("%(levelname)s - %(message)s")
    handler.setFormatter(formatter)
    logger.addHandler(handler)
    return logger

Log = getLogger()
LOCK_FILE = "/var/run/keybot.lock"

class LockMaster(object):
    def __init__(self, lock_name, block=True):
        self.LockName = lock_name
        self.Block = block

    def __enter__(self):
        with open(self.LockName, 'a+') as fp:
            if self.Block is True:
                fcntl.flock(fp, fcntl.LOCK_EX)
            else:
                fcntl.flock(fp, fcntl.LOCK_EX | fcntl.LOCK_NB)
            Log.debug("Acquired lock on {}.".format(self.LockName))

    def __exit__(self, type, value, traceback):
        with open(self.LockName, 'a+') as fp:
            fcntl.flock(fp, fcntl.LOCK_UN)
        Log.debug("Released lock on {}".format(self.LockName))

class ConfigParams(object):
    def __init__(self):
        self.Quiet = False
        self.Welcome = ""
        self.Admins = []
        self.Token = ""
        self.Username = ""
        self.LogDebug = False
        self.WorkerCount = 4

        self.RedditClientID = ""
        self.RedditClientSecret = ""
        self.RedditDailyPicCaption = ""

    @classmethod
    def loadFromFile(cls, filename):
        if not os.path.exists(filename):
            Log.error("Config file not exist at {}.".format(filename))

        Config = ConfigParams()
        Parser = configparser.ConfigParser()
        Parser.read(filename)

        if "General" in Parser:
            Sec = Parser["General"]
            Config.Quiet = Sec.getboolean("Quiet", fallback=Config.Quiet)
            Config.Admins = Sec.get("Admins", "").split(',')
            Config.Welcome = Sec.get("Welcome", Config.Welcome)
            Config.Token = Sec.get("Token", Config.Token)
            Config.Username = Sec.get("Username", Config.Username)
            Config.LogDebug = Sec.getboolean("LogDebug", fallback=Config.LogDebug)
            Config.WorkerCount = int(Sec.get("WorkerCount", str(Config.WorkerCount)))

        if "Reddit" in Parser:
            Sec = Parser["Reddit"]
            Config.RedditClientID = Sec.get("ClientID", Config.RedditClientID)
            Config.RedditClientSecret = Sec.get("ClientSecret",
                                                Config.RedditClientSecret)
            Config.RedditDailyPicCaption = Sec.get("DailyPicCaption",
                                                   Config.RedditDailyPicCaption)

        return Config

class RuntimeInfo(object):
    File = "runtime-info.json"
    def __init__(self):
        pass

    def getDict(self):
        if not os.path.exists(self.File):
            Log.error("Runtime info file {} does not exist.".format(self.File))
            return {}

        with open(self.File, 'r') as f:
            return json.load(f)

    def get(self, key, fallback_value=None):
        return self.getDict().get(key, fallback_value)

    def set(self, key, value):
        Data = self.getDict()
        Data[key] = value
        with LockMaster(LOCK_FILE) as _:
            try:
                with open(self.File, 'w') as f:
                    json.dump(Data, f)
            except Exception:
                Logger.error("Failed to write runtime info file at {}."
                             .format(self.File))

# Not used
@run_async
def sendAsync(bot, *args, **kwargs):
    bot.send_message(*args, **kwargs);

# Not used.
def check(bot, config, update, override_lock=None):
    """Perform some checks on the update. If checks were successful, returns True,
    else sends an error message to the chat and returns False.
    """

    ChatID = update.message.chat_id
    ChatStr = str(ChatID)

    if ChatID > 0:
        sendAsync(bot, chat_id=ChatID,
                   text='Please add me to a group first!')
        return False

    if config.Quiet:
        return False
    return True

def welcome(bot, config, new_users, chat):
    """Welcomes a user to the chat."""
    if config.Quiet is True:
        Log.debug("Quiet! Don't welcome!")
        return

    MsgTplt = string.Template(config.Welcome)
    Log.debug("Welcoming {}...".format(', '.join(u.name for u in new_users)))

    for Member in new_users:
        Log.info('{} joined to chat {} ({})'.format(
            escape(Member.name), chat.id, escape(chat.title)))

        # Pull the custom message for this chat from the database
        Msg = MsgTplt.safe_substitute(
            user="[{}](tg://user?id={})".format(escape(Member.full_name), Member.id))

        # Use default message if there's no custom one set
        if Msg == "":
            return

        bot.send_message(chat_id=chat.id, text=Msg,
                         parse_mode=telegram.ParseMode.MARKDOWN)

def onEmptyMsg(bot, config, update):
    """Empty messages could be status messages, so we check them if there is a new
    group member, someone left the chat or if the bot has been added somewhere.
    """
    Chat = update.message.chat
    Log.debug("Got status update from chat {}.".format(Chat.id))
    RuntimeInfo().set("ChatID", Chat.id)

    NewMembers = update.message.new_chat_members
    if NewMembers:
        Names = tuple(escape(user.username) for user in NewMembers)
        Log.debug("Status update for {}.".format(', '.join(Names)))
        # Bot was added to a group chat
        if config.Username in Names:
            Log.info("Added to chat {} ({}).".format(Chat.id, Chat.title))
        # Another user joined the chat
        else:
            Log.debug("New user joined.")
            try:
                welcome(bot, config, NewMembers, Chat)
            except Exception:
                Log.exception("Failed to welcome.")

def onError(bot, update, error):
    Log.error("{}\n{}".format(error, repr(error)))

def getRedditPostsToday(config: ConfigParams) -> typing.List[reddit.RedditPost]:
    Log.debug("Authenticating on Reddit...")
    reddit.RedditQuery.authenticateUserLess(config.RedditClientID,
                                            config.RedditClientSecret)

    Sub = reddit.SubredditInfo("MechanicalKeyboards")
    Posts = []
    Log.debug("Getting post...")
    Posts = Sub.list(reddit.PostSorting.New)
    Now = datetime.datetime.now()
    Yesterday = Now - datetime.timedelta(days=1)
    while Posts[-1].TimeCreate > Yesterday:
        Posts += Sub.list(reddit.PostSorting.New, after=Posts[-1].FullName)

    for i in range(-1, -1 * (len(Posts) + 1), -1):
        Post = Posts[i]
        if Post.TimeCreate > Yesterday:
            Posts = Posts[:i]
            break

    Log.debug("Logging out on Reddit...")
    reddit.RedditQuery.logout(config.RedditClientID, config.RedditClientSecret)
    return Posts

def sendBestRedditToday(config):
    ChatID = RuntimeInfo().get("ChatID")
    if ChatID is None:
        return

    Posts = getRedditPostsToday(config)
    BestPost = max((p for p in Posts if p.IsLink and
                    (p.Link.endswith(".jpg") or p.Link.endswith(".jpeg") or
                     p.Link.endswith(".png"))),
                   key=lambda p: p.Score)
    Log.info("Best reddit post today is " + BestPost.ShortUrl)

    Updater = BotAPI.Updater(config.Token, workers=1)
    CapTplt = string.Template(config.RedditDailyPicCaption)
    Updater.bot.send_photo(ChatID, BestPost.Link,
                           CapTplt.safe_substitute(url=BestPost.ShortUrl),
                           parse_mode=telegram.ParseMode.MARKDOWN)

def onCMDTest(bot, config, update):
    Log.info("Test command issued from {}.".format(update.message.from_user.full_name))
    Posts = getRedditPostsToday(config)
    Log.debug("Got {} posts.".format(len(Posts)))
    BestPost = max((p for p in Posts if p.IsLink and
                    (p.Link.endswith(".jpg") or p.Link.endswith(".jpeg") or
                     p.Link.endswith(".png"))),
                   key=lambda p: p.Score)
    Log.info("Best reddit post today is " + BestPost.ShortUrl)

    CapTplt = string.Template(config.RedditDailyPicCaption)
    bot.send_photo(update.message.from_user.id, BestPost.Link,
                   CapTplt.safe_substitute(url=BestPost.ShortUrl),
                   parse_mode=telegram.ParseMode.MARKDOWN)

def onCMDSecretTest(bot, config, update):
    sendBestRedditToday(config)

def startBot(config: ConfigParams):
    Updater = BotAPI.Updater(config.Token, workers=config.WorkerCount)
    Dispatch= Updater.dispatcher

    Log.info("Adding handlers...")
    Dispatch.add_handler(BotAPI.MessageHandler(
        BotAPI.Filters.status_update,
        lambda bot, update: onEmptyMsg(bot, config, update)))

    Dispatch.add_error_handler(onError)

    # Commands
    Dispatch.add_handler(BotAPI.CommandHandler(
        "test", lambda bot, update: onCMDTest(bot, config, update)))

    Dispatch.add_handler(BotAPI.CommandHandler(
        "secrettest", lambda bot, update: onCMDSecretTest(bot, config, update)))

    Log.info("Starting to poll...")
    UpdateQueue = Updater.start_polling(timeout=30, clean=False)
    Updater.idle()
