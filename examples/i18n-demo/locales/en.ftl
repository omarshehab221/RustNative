# The window's heading.
title = Inbox
# How many messages are waiting.
inbox-count = { $count ->
    [0] Your inbox is empty.
    [one] You have one message.
   *[other] You have { $count } messages.
}
# Who invited the person, by the inviter's grammatical gender.
invited = { $gender ->
    [feminine] { $name } invited you to her team.
    [masculine] { $name } invited you to his team.
   *[other] { $name } invited you to their team.
}
# The button that adds a message.
more = One more
# The button that removes one.
fewer = One fewer
# The total, formatted by the host for the locale.
total = Total: { $amount }
