#!/usr/bin/env python

import sys
import os
import logging
import keybot

Log = keybot.Log

def readConfig():
    ConfigFile = "keybot.ini"
    Log.info("Reading config from {}...".format(os.path.abspath(ConfigFile)))
    Config = keybot.ConfigParams.loadFromFile(ConfigFile)
    if Config.LogDebug is True:
        Log.setLevel(logging.DEBUG)

    Log.debug("Username: " + Config.Username)
    Log.debug("Welcome: " + Config.Welcome)
    Log.debug("DoWelcome: " + str(Config.DoWelcome))
    Log.debug("WorkerCount: " + str(Config.WorkerCount))
    Log.debug("RedditDailyPicCaption: " + str(Config.RedditDailyPicCaption))

    return Config

def main():
    import argparse

    Parser = argparse.ArgumentParser(description="Keybot!")
    Parser.add_argument('--send-reddit-best', dest='SendRedditBest', default=False,
                        action="store_true",
                        help="Send Reddit's best pic today to group.")

    Args = Parser.parse_args()

    Config = readConfig()

    if Args.SendRedditBest is True:
        keybot.sendBestRedditToday(Config)
        return 0

    keybot.startBot(Config)
    return 0

if __name__ == "__main__":
    sys.exit(main())
