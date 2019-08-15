#!/usr/bin/env python

import sys, os
import typing
import logging
import configparser
import string
import datetime
import json
import fcntl
import subprocess
import tempfile
from html import escape

import requests

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
LOCK_FILE = "/tmp/keybot.lock"
TG_IMG_SIZE_LIMIT = 4096
TG_IMG_FILE_SIZE_LIMIT = 5 * 1024 * 1024
IMG_RESIZE_TARGET = 1024
IMG_RESIZE_QUALITY = 92

def getTempFile(suffix="") -> str:
    t, Temp = tempfile.mkstemp(prefix="keybot-", suffix=suffix)
    os.close(t)
    Log.debug("Using temp file {}.".format(Temp))
    return Temp

class LockMaster(object):
    def __init__(self, lock_name, block=True):
        self.LockName = lock_name
        self.Block = block

    def __enter__(self):
        try:
            with open(self.LockName, 'a+') as fp:
                if self.Block is True:
                    fcntl.flock(fp, fcntl.LOCK_EX)
                else:
                    fcntl.flock(fp, fcntl.LOCK_EX | fcntl.LOCK_NB)
                Log.debug("Acquired lock on {}.".format(self.LockName))
        except Exception:
            Log.error("Unable to acquire lock at {}.".format(LOCK_FILE))

    def __exit__(self, type, value, traceback):
        with open(self.LockName, 'a+') as fp:
            fcntl.flock(fp, fcntl.LOCK_UN)
        Log.debug("Released lock on {}".format(self.LockName))

class ConfigParams(object):
    def __init__(self):
        self.DoWelcome = True
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
            Config.DoWelcome = Sec.getboolean("DoWelcome", fallback=Config.DoWelcome)
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

    def saveToFile(self, filename):
        Parser = configparser.ConfigParser()
        Parser.add_section("General")
        Parser.set("General", "DoWelcome", self.DoWelcome)
        Parser.set("General", "Admins", ','.join(self.Admins))
        Parser.set("General", "Welcome", self.Welcome)
        Parser.set("General", "Token", self.Token)
        Parser.set("General", "Username", self.Username)
        Parser.set("General", "LogDebug", self.LogDebug)
        Parser.set("General", "WorkerCount", self.WorkerCount)

        if self.RedditClientID != "":
            Parser.add_section("Reddit")
            Parser.set("Reddit", "ClientID", self.RedditClientID)
            Parser.set("Reddit", "ClientSecret", self.RedditClientSecret)
            Parser.set("Reddit", "DailyPicCaption", self.RedditDailyPicCaption)

        with open(filename, 'w') as f:
            Parser.write(f)

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

    if not config.DoWelcome:
        return False
    return True

def welcome(bot, config, new_users, chat):
    """Welcomes a user to the chat."""
    if config.DoWelcome is False:
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

def downloadFile(uri, to_file):
    subprocess.check_call(["curl", "--silent", "-o", to_file, uri])

def getURIFileSize(uri: str) -> typing.Tuple[int, str]:
    """Get the file size pointed by `uri`. This may download the file. If it does,
    return a tuple (size, downloaded filename), otherwise return (size, None).
    """
    Res = requests.head(uri)
    Res.raise_for_status()
    if "Content-Length" in Res.headers:
        Log.debug("Got file size from header: " + str(Res.headers["Content-Length"]))
        return (int(Res.headers["Content-Length"]), None)
    else:
        Temp = getTempFile()
        Log.info("Downloading {} into {}...".format(uri, Temp))
        downloadFile(uri, Temp)
        Size = os.path.getsize(Temp)
        return (Size, Temp)

def sendPhoto(bot, uri, caption, chat_id):
    """Send photo at `uri`. If the photo is too big, resize it and send. This always
    downloads the file to disk.
    """
    Log.debug("Sending photo at {}...".format(uri))
    try:
        SizeStr = subprocess.check_output(["magick", "identify", "-ping", "-format",
                                           "%wx%h", uri])
    except Exception as Err:
        Log.exception("Failed to get image size: " + repr(Err))
        return
    Log.debug("Identify printed {}.".format(SizeStr))

    Parts = SizeStr.decode().split('x')
    ImgSize = (int(Parts[0]), int(Parts[1]))

    Log.debug("Image size is {}.".format(ImgSize))

    FileSize, File = getURIFileSize(uri)
    Log.debug("File size for {} is {}.".format(File, FileSize))

    if ImgSize[0] < TG_IMG_SIZE_LIMIT and ImgSize[1] < TG_IMG_SIZE_LIMIT \
       and FileSize < TG_IMG_FILE_SIZE_LIMIT:
        # Image is small. Just send URI.
        bot.send_photo(chat_id, uri, caption,
                       parse_mode=telegram.ParseMode.MARKDOWN)
    else:
        Log.info("Processing large image file...")
        # Image is large. Download...
        if File is None:
            File = getTempFile()
            downloadFile(uri, File)

        # ... and resize
        Log.info("Resizing image to {}...".format(IMG_RESIZE_TARGET))
        NewFile = getTempFile(".jpg")
        try:
            # Due to the limitation of the VPS, this is surprisingly easy to
            # fail.
            subprocess.check_call(["magick", "convert", File,
                                   "-limit", "memory", "100MiB",
                                   # "-limit", "map", "200MiB",
                                   "-resize", "{0}x{0}".format(IMG_RESIZE_TARGET),
                                   "-quality", str(IMG_RESIZE_QUALITY),
                                   NewFile])
        except subprocess.CalledProcessError:
            Log.error("Failed to resize image.")
            os.remove(File)
            raise
        os.remove(File)

        try:
            with open(NewFile, 'rb') as ImgFile:
                bot.send_photo(chat_id, ImgFile, caption,
                               parse_mode=telegram.ParseMode.MARKDOWN)
        finally:
            os.remove(NewFile)

def trySendFirstPhotoFromPosts(bot, chat_id, posts, caption_tplt):
    for BestPost in posts:
        Log.info("Best reddit post today is {}, with image at {}.".format(
            BestPost.ShortUrl, BestPost.Link))

        try:
            sendPhoto(bot, BestPost.Link,
                      caption_tplt.safe_substitute(url=BestPost.ShortUrl),
                      chat_id)
        except subprocess.CalledProcessError as Err:
            Log.exception("Failed to process image. Trying the next best...")
            continue
        except Exception as Err:
            Log.exception(Err)
        else:
            break

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
    BestPosts = sorted((p for p in Posts if p.IsLink and
                       (p.Link.endswith(".jpg") or p.Link.endswith(".jpeg") or
                        p.Link.endswith(".png"))),
                      key=lambda p: p.Score, reverse=True)

    Updater = BotAPI.Updater(config.Token, workers=1)
    CapTplt = string.Template(config.RedditDailyPicCaption)

    trySendFirstPhotoFromPosts(Updater.bot, ChatID, BestPosts, CapTplt)

def onCMDTest(bot, config, update):
    Log.info("Test command issued from {}.".format(update.message.from_user.full_name))
    Posts = getRedditPostsToday(config)
    Log.debug("Got {} posts.".format(len(Posts)))
    BestPosts = sorted((p for p in Posts if p.IsLink and
                       (p.Link.endswith(".jpg") or p.Link.endswith(".jpeg") or
                        p.Link.endswith(".png"))),
                      key=lambda p: p.Score, reverse=True)

    CapTplt = string.Template(config.RedditDailyPicCaption)
    trySendFirstPhotoFromPosts(bot, update.message.from_user.id, BestPosts,
                               CapTplt)

def onCMDSecretTest(bot, config, update):
    sendBestRedditToday(config)

def onCMDPing(bot, update):
    update.message.reply_text("Pong!")

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
    Dispatch.add_handler(BotAPI.CommandHandler("ping", onCMDPing))

    Log.info("Starting to poll...")
    UpdateQueue = Updater.start_polling(timeout=30, clean=False)
    Updater.idle()
