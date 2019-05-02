#!/usr/bin/env python

import sys, os
import logging
import configparser
import string
from html import escape

import telegram
import telegram.ext as BotAPI
from telegram.ext.dispatcher import run_async # Not used

def get_logger(name=__name__, level=logging.INFO):
    logger = logging.getLogger(name)
    logger.setLevel(level)
    handler = logging.StreamHandler(sys.stderr)
    formatter = logging.Formatter("%(levelname)s - %(message)s")
    handler.setFormatter(formatter)
    logger.addHandler(handler)
    return logger

Log = get_logger()

class ConfigParams(object):
    def __init__(self):
        self.Quiet = False
        self.Welcome = ""
        self.Admins = []
        self.Token = ""
        self.Username = ""
        self.LogDebug = False
        self.WorkerCount = 4

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

        return Config

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

def main():
    ConfigFile = "keybot.ini"
    Log.info("Reading config from {}...".format(os.path.abspath(ConfigFile)))
    Config = ConfigParams.loadFromFile(ConfigFile)
    if Config.LogDebug is True:
        Log.setLevel(logging.DEBUG)

    Log.debug("Username: " + Config.Username)
    Log.debug("Welcome: " + Config.Welcome)
    Log.debug("Quiet: " + str(Config.Quiet))
    Log.debug("WorkerCount: " + str(Config.WorkerCount))

    Updater = BotAPI.Updater(Config.Token, workers=Config.WorkerCount)
    Dispatch= Updater.dispatcher

    Log.info("Adding handlers...")
    Dispatch.add_handler(BotAPI.MessageHandler(
        BotAPI.Filters.status_update,
        lambda bot, update: onEmptyMsg(bot, Config, update)))

    Dispatch.add_error_handler(onError)

    Log.info("Starting to poll...")
    UpdateQueue = Updater.start_polling(timeout=30, clean=False)
    Updater.idle()

if __name__ == '__main__':
    main()
