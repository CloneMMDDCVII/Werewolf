using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Threading.Tasks;

namespace Werewolf_Control
{
    public static partial class Commands
    {

		[Command(Trigger = "gamelist")]
		internal static void gamelist(Update update, string[] args)
		{
		            var reply = "";
            using (var db = new WWContext())
            {
                reply = Enumerable.Aggregate(db.v_PreferredGroups, "", (current, g) => current + $"{GetLanguageName(g.Language)}{(String.IsNullOrEmpty(g.Description) ? "" : $" - {g.Description}")}\n<a href=\"{g.GroupLink}\">{g.Name}</a>\n\n");
            }
            Send(reply, update.Message.From.Id);

		}

		[Command(Trigger = "rolelist")]
		internal static void getroles(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("/AboutVG - Villager\n/AboutSeer - Seer\n/AboutWw - Werewolf\n/AboutHarlot - Harlot\n/AboutDrunk - Drunk\n/AboutCursed - Cursed\n/AboutTraitor - Traitor\n/AboutGA - Guardian Angel\n/AboutDetective - Detective\n/AboutGunner - Gunner\n/AboutTanner - Tanner\n/AboutFool - Fool\n/AboutCult - Cultist\n/AboutCH - Cultist Hunter\n/AboutWC - Wild Child\n/AboutAppS - Apprentice seer\n/AboutBH - Beholder\n/AboutMason - Mason\n/AboutDG - Doppelgänger\n/AboutCupid - Cupid\n/AboutHunter - Hunter\n/AboutSK - Serial Killer"), update.Message.From.Id);

		}

		[Command(Trigger = "AboutVG")]
		internal static void AboutVG(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutVG", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutSeer")]
		internal static void AboutSeer(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutSeer", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutWw")]
		internal static void AboutWw(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutWw", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutHarlot")]
		internal static void AboutHarlot(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutHarlot", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutDrunk")]
		internal static void AboutDrunk(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutDrunk", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutCursed")]
		internal static void AboutCursed(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutCursed", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutTraitor")]
		internal static void AboutTraitor(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutTraitor", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutGA")]
		internal static void AboutGA(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutGA", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutDetective")]
		internal static void AboutDetective(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutDetective", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutHolmes")]
		internal static void AboutDetective(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutDetective", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutGunner")]
		internal static void AboutGunner(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutGunner", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutTanner")]
		internal static void AboutTanner(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutTanner", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutFool")]
		internal static void AboutFool(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutFool", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutCult")]
		internal static void AboutCult(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutCult", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutCH")]
		internal static void AboutCH(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutCH", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutWC")]
		internal static void AboutWC(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutWC", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutAppS")]
		internal static void AboutAppS(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutAppS", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutBH")]
		internal static void AboutBH(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutBH", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutMason")]
		internal static void AboutMason(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutMason", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutDG")]
		internal static void AboutDG(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutDG", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutCupid")]
		internal static void AboutCupid(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutCupid", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutHunter")]
		internal static void AboutHunter(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutHunter", lang), update.Message.From.Id);

		}

		[Command(Trigger = "AboutSK")]
		internal static void AboutSK(Update update, string[] args)
		{
		var lang = GetLanguage(update.Message.Chat.Id);
            Send(GetLocaleString("AboutSK", lang), update.Message.From.Id);

		}





    }
}
