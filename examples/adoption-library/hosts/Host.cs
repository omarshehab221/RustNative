// A C# host: an existing .NET application adopting a Rust Native model
// through the generated bindings (`rustnative bindgen counter.ril --lang csharp`).
// Exits 0 when every expectation holds.
using System;
using Counter.Interop;

public static class Host
{
    private static void Expect(bool condition, string what)
    {
        if (!condition) throw new Exception("failed: " + what);
    }

    public static int Main()
    {
        try
        {
            int changes = 0;
            uint last = 0;
            using (Counter.Interop.Counter counter = new Counter.Interop.Counter(40))
            {
                counter.OnChanged(count => { changes++; last = count; });
                counter.Increment();
                Expect(counter.Increment() == 42, "the count is 42");
                Expect(changes == 2 && last == 42, "changed was raised twice");
                counter.Rename("Zählwerk");
                Expect(counter.Label() == "Zählwerk: 42", "the label round-trips UTF-8: " + counter.Label());
                try
                {
                    counter.Rename(null);
                    Expect(false, "a null name is refused");
                }
                catch (ArgumentNullException)
                {
                }
            }
            Console.WriteLine("ok");
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.Message);
            return 1;
        }
    }
}
