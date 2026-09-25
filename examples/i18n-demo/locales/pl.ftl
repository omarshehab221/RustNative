title = Skrzynka odbiorcza
inbox-count = { $count ->
    [0] Skrzynka jest pusta.
    [one] Masz jedną wiadomość.
    [few] Masz { $count } wiadomości.
   *[many] Masz { $count } wiadomości.
}
invited = { $gender ->
    [feminine] { $name } zaprosiła cię do swojego zespołu.
    [masculine] { $name } zaprosił cię do swojego zespołu.
   *[other] { $name } zaprosiło cię do swojego zespołu.
}
more = Jeszcze jedna
fewer = O jedną mniej
total = Razem: { $amount }
