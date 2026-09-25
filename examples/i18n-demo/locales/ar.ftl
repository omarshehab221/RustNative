title = البريد الوارد
inbox-count = { $count ->
    [zero] صندوق الوارد فارغ.
    [one] لديك رسالة واحدة.
    [two] لديك رسالتان.
    [few] لديك { $count } رسائل.
    [many] لديك { $count } رسالة.
   *[other] لديك { $count } رسالة.
}
invited = { $gender ->
    [feminine] دعتك { $name } إلى فريقها.
    [masculine] دعاك { $name } إلى فريقه.
   *[other] دعاك { $name } إلى الفريق.
}
more = رسالة أخرى
fewer = رسالة أقل
total = المجموع: { $amount }
