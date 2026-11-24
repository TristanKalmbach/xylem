// Runs XSLT stylesheets the way a .NET publishing service that hosts
// Saxon-HE (9.x, .NET edition) does, so output produced here can be compared
// byte for byte with output that service saved.
//
// What it copies from such a host:
//   - XSLT 2.0 stylesheets (root version="2.0") run on Saxon: a new Processor
//     per stylesheet, compiled from the stylesheet's file URI, so imports,
//     includes and document() resolve relative to it.
//   - The input's base URI is the stylesheet's folder without a trailing
//     slash (Uri(FileInfo.DirectoryName)).
//   - Parameters are matched case-insensitively against the declared
//     xsl:params and converted to the declared type; undeclared ones are
//     passed as strings.
//   - The result goes to a .NET XmlDocument (DomDestination) and is written
//     as its OuterXml, UTF-8 with a byte order mark. xsl:output is ignored.
//   - Other stylesheets run on XslCompiledTransform with document() and
//     scripts enabled, DTDs parsed, the input read from a string with no base
//     URI, and the XML declaration omitted.
//
// Usage:
//   SaxonDotnetHost run --stylesheet <file.xsl> --job <input.xml>=<output>
//                       [--job ...] [--param name=value ...] [--profile]
//
// --repeat N runs each job N times and reports the median time ("ms") and
// every run's time ("runs_ms"), for comparing speed before and after a
// change. The output is written by every run.
//
//   SaxonDotnetHost compile --stylesheet <file.xsl> [--stylesheet ...]
//
// compiles each stylesheet as the host would and prints one JSON line per
// stylesheet ({"stylesheet":…,"ok":…,"ms":…,"error":…}); the exit code is 0
// only when all compiled.
//
// --profile times every template and function (XSLT 2.0 only) and writes
// <output>.profile.json next to each output: per template its kind, name or
// match pattern, mode, file and line, how often it ran, the time spent in it
// alone ("self_ms") and including what it called ("total_ms"). Timing hooks
// slow the run down, so times are for comparing templates with each other;
// the output itself is unchanged.
//
// Each job reads an input file and writes the output file. The stylesheet is
// compiled once for all jobs. One line of JSON per job goes to standard
// output, ending with a summary line:
//   {"job":1,"input":"...","output":"...","ok":true,"ms":1234}
//   {"summary":true,"xslt":"2.0","compile_ms":456,"jobs":1,"failed":0}
// Errors go to standard error; the exit code is 0 only when every job worked.
//
// Built for .NET Framework 4 with the C# 5 compiler that ships with Windows
// (see build.cmd), against the Saxon-HE .NET assemblies the host uses.

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Xml;
using System.Xml.Linq;
using System.Xml.Xsl;
using Saxon.Api;

namespace Xylem.SaxonDotnet
{
    internal sealed class Job
    {
        public string Input;
        public string Output;
    }

    internal sealed class Options
    {
        public string Stylesheet;
        public bool Profile;
        public bool CompileOnly;
        public int Repeat = 1;
        public readonly List<string> Stylesheets = new List<string>();
        public readonly List<Job> Jobs = new List<Job>();
        public readonly List<KeyValuePair<string, string>> Params = new List<KeyValuePair<string, string>>();
    }

    internal static class Program
    {
        private static int Main(string[] args)
        {
            Options options;
            try
            {
                options = ParseArgs(args);
            }
            catch (ArgumentException e)
            {
                Console.Error.WriteLine(e.Message);
                Console.Error.WriteLine("Usage: SaxonDotnetHost run --stylesheet <file.xsl> --job <input.xml>=<output> [--job ...] [--param name=value ...]");
                return 2;
            }

            if (options.CompileOnly)
                return CompileAll(options.Stylesheets);
            var stylesheet = Path.GetFullPath(options.Stylesheet);
            bool isXslt2;
            try
            {
                isXslt2 = IsXslt2(stylesheet);
            }
            catch (Exception e)
            {
                Console.Error.WriteLine("Could not read the stylesheet " + stylesheet + ": " + e.Message);
                return 1;
            }

            var compileClock = Stopwatch.StartNew();
            ITransform transform;
            try
            {
                transform = isXslt2
                    ? (ITransform)new SaxonTransform(stylesheet, options.Profile)
                    : new DotnetTransform(stylesheet);
            }
            catch (Exception e)
            {
                Console.Error.WriteLine("The stylesheet did not compile: " + Describe(e));
                Console.WriteLine("{\"summary\":true,\"xslt\":" + Json(isXslt2 ? "2.0" : "1.0") + ",\"compiled\":false,\"error\":" + Json(Describe(e)) + "}");
                return 1;
            }
            compileClock.Stop();

            int failed = 0;
            for (int i = 0; i < options.Jobs.Count; i++)
            {
                var job = options.Jobs[i];
                var clock = Stopwatch.StartNew();
                string error = null;
                string profile = options.Profile && isXslt2 ? Path.GetFullPath(job.Output) + ".profile.json" : null;
                var runs = new List<long>();
                try
                {
                    for (int r = 0; r < Math.Max(1, options.Repeat); r++)
                    {
                        var runClock = Stopwatch.StartNew();
                        var result = transform.Run(Path.GetFullPath(job.Input), options.Params, profile);
                        runClock.Stop();
                        runs.Add(runClock.ElapsedMilliseconds);
                        var outputDir = Path.GetDirectoryName(Path.GetFullPath(job.Output));
                        if (!string.IsNullOrEmpty(outputDir))
                            Directory.CreateDirectory(outputDir);
                        // As the host writes it: UTF-8 with a byte order mark.
                        File.WriteAllText(job.Output, result, Encoding.UTF8);
                    }
                }
                catch (Exception e)
                {
                    error = Describe(e);
                    failed++;
                    Console.Error.WriteLine(job.Input + ": " + error);
                }
                clock.Stop();
                var line = new StringBuilder();
                line.Append("{\"job\":").Append(i + 1)
                    .Append(",\"input\":").Append(Json(job.Input))
                    .Append(",\"output\":").Append(Json(job.Output))
                    .Append(",\"ok\":").Append(error == null ? "true" : "false")
                    .Append(",\"ms\":").Append(runs.Count > 1 ? Median(runs) : clock.ElapsedMilliseconds);
                if (runs.Count > 1)
                    line.Append(",\"runs_ms\":[").Append(string.Join(",", runs)).Append(']');
                if (profile != null && error == null)
                    line.Append(",\"profile\":").Append(Json(profile));
                if (error != null)
                    line.Append(",\"error\":").Append(Json(error));
                line.Append('}');
                Console.WriteLine(line.ToString());
            }

            Console.WriteLine("{\"summary\":true,\"xslt\":" + Json(isXslt2 ? "2.0" : "1.0")
                + ",\"compiled\":true,\"compile_ms\":" + compileClock.ElapsedMilliseconds
                + ",\"jobs\":" + options.Jobs.Count + ",\"failed\":" + failed + "}");
            return failed == 0 ? 0 : 1;
        }

        private static long Median(List<long> values)
        {
            var sorted = values.OrderBy(v => v).ToList();
            return sorted[sorted.Count / 2];
        }

        /// <summary>Compiles each stylesheet as a run would.</summary>
        private static int CompileAll(List<string> stylesheets)
        {
            int failed = 0;
            foreach (var s in stylesheets)
            {
                var full = Path.GetFullPath(s);
                var clock = Stopwatch.StartNew();
                string error = null;
                try
                {
                    if (IsXslt2(full))
                        new SaxonTransform(full, false);
                    else
                        new DotnetTransform(full);
                }
                catch (Exception e)
                {
                    error = Describe(e);
                    failed++;
                }
                clock.Stop();
                var line = new StringBuilder();
                line.Append("{\"stylesheet\":").Append(Json(s))
                    .Append(",\"ok\":").Append(error == null ? "true" : "false")
                    .Append(",\"ms\":").Append(clock.ElapsedMilliseconds);
                if (error != null)
                    line.Append(",\"error\":").Append(Json(error));
                line.Append('}');
                Console.WriteLine(line.ToString());
            }
            return failed == 0 ? 0 : 1;
        }

        private static Options ParseArgs(string[] args)
        {
            if (args.Length == 0 || (args[0] != "run" && args[0] != "compile"))
                throw new ArgumentException("The first argument must be 'run' or 'compile'.");
            var options = new Options { CompileOnly = args[0] == "compile" };
            for (int i = 1; i < args.Length; i++)
            {
                string value = i + 1 < args.Length ? args[i + 1] : null;
                switch (args[i])
                {
                    case "--stylesheet":
                        options.Stylesheet = Require(args[i], value);
                        options.Stylesheets.Add(options.Stylesheet);
                        i++;
                        break;
                    case "--repeat":
                        {
                            int n;
                            if (!int.TryParse(Require(args[i], value), out n) || n < 1)
                                throw new ArgumentException("--repeat takes a number of runs.");
                            options.Repeat = n;
                            i++;
                            break;
                        }
                    case "--job":
                        {
                            var pair = Require(args[i], value);
                            int eq = pair.IndexOf('=');
                            if (eq <= 0 || eq == pair.Length - 1)
                                throw new ArgumentException("--job takes <input>=<output>, got: " + pair);
                            options.Jobs.Add(new Job { Input = pair.Substring(0, eq), Output = pair.Substring(eq + 1) });
                            i++;
                            break;
                        }
                    case "--profile":
                        options.Profile = true;
                        break;
                    case "--param":
                        {
                            var pair = Require(args[i], value);
                            int eq = pair.IndexOf('=');
                            if (eq <= 0)
                                throw new ArgumentException("--param takes name=value, got: " + pair);
                            options.Params.Add(new KeyValuePair<string, string>(pair.Substring(0, eq), pair.Substring(eq + 1)));
                            i++;
                            break;
                        }
                    default:
                        throw new ArgumentException("Unknown option: " + args[i]);
                }
            }
            if (options.Stylesheet == null)
                throw new ArgumentException("--stylesheet is required.");
            if (options.CompileOnly)
                return options;
            if (options.Jobs.Count == 0)
                throw new ArgumentException("At least one --job is required.");
            return options;
        }

        private static string Require(string option, string value)
        {
            if (value == null)
                throw new ArgumentException(option + " needs a value.");
            return value;
        }

        /// <summary>As the host decides: the root's version attribute is exactly "2.0".</summary>
        private static bool IsXslt2(string stylesheet)
        {
            using (var s = new FileStream(stylesheet, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
            {
                var doc = XDocument.Load(s);
                var version = doc.Root.Attribute("version");
                return version != null && version.Value == "2.0";
            }
        }

        private static string Describe(Exception e)
        {
            var parts = new List<string>();
            for (var cur = e; cur != null; cur = cur.InnerException)
                parts.Add(cur.Message);
            return string.Join(" / ", parts.Distinct());
        }

        internal static string Json(string s)
        {
            var sb = new StringBuilder("\"");
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': sb.Append("\\\""); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default:
                        if (c < ' ')
                            sb.Append("\\u").Append(((int)c).ToString("x4"));
                        else
                            sb.Append(c);
                        break;
                }
            }
            return sb.Append('"').ToString();
        }
    }

    internal interface ITransform
    {
        string Run(string inputPath, List<KeyValuePair<string, string>> parameters, string profilePath);
    }

    /// <summary>XSLT 2.0 on Saxon-HE, as the host runs it.</summary>
    internal sealed class SaxonTransform : ITransform
    {
        private readonly string _stylesheet;
        private readonly XsltExecutable _executable;
        private readonly Processor _processor;

        /// <summary>
        /// The stylesheet's own xsl:strip-space/xsl:preserve-space rules as a
        /// document builder policy. The .NET API has this constructor but
        /// does not make it public.
        /// </summary>
        private static WhitespacePolicy StylesheetWhitespace(XsltExecutable executable)
        {
            var prepared = typeof(XsltExecutable)
                .GetProperty("Implementation", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic)
                .GetValue(executable, null);
            var ctor = typeof(WhitespacePolicy).GetConstructor(
                System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Public,
                null, new[] { prepared.GetType() }, null);
            if (ctor == null)
                throw new InvalidOperationException("This Saxon version cannot strip whitespace as the stylesheet asks.");
            return (WhitespacePolicy)ctor.Invoke(new[] { prepared });
        }

        public SaxonTransform(string stylesheet, bool profile)
        {
            _stylesheet = stylesheet;
            var processor = new Processor();
            _processor = processor;
            if (profile)
                processor.Implementation.setCompileWithTracing(true);
            var compiler = processor.NewXsltCompiler();
            _executable = compiler.Compile(new Uri(stylesheet));
        }

        public string Run(string inputPath, List<KeyValuePair<string, string>> parameters, string profilePath)
        {
            var declared = _executable.GetGlobalParameters();
            TimingListener timing = null;
            var destination = new DomDestination();
            // The host saves its XDocument to a stream and hands Saxon those
            // bytes; the saved input file holds the same bytes.
            using (var input = new FileStream(inputPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
            {
                var transformer = _executable.Load();
                var stylesheetFolder = new FileInfo(_stylesheet).DirectoryName;
                if (profilePath == null)
                {
                    transformer.SetInputStream(input, new Uri(stylesheetFolder));
                }
                else
                {
                    // With timing hooks compiled in, this Saxon version
                    // evaluates global variables before a streamed input is
                    // attached, so the tree is built first, the way the
                    // transform itself would (whitespace stripped as the
                    // stylesheet asks), keeping node numbers the same.
                    var builder = _processor.NewDocumentBuilder();
                    builder.BaseUri = new Uri(stylesheetFolder);
                    builder.WhitespacePolicy = StylesheetWhitespace(_executable);
                    transformer.InitialContextNode = builder.Build(input);
                }
                foreach (var arg in parameters)
                    SetParameter(transformer, declared, arg.Key, arg.Value);
                if (profilePath != null)
                {
                    timing = new TimingListener();
                    transformer.Implementation.addTraceListener(timing);
                }
                transformer.Run(destination);
            }
            if (timing != null)
                File.WriteAllText(profilePath, timing.ToJson(), new UTF8Encoding(false));
            return destination.XmlDocument.OuterXml;
        }

        private static void SetParameter(XsltTransformer transformer, Dictionary<QName, XsltExecutable.ParameterDetails> declared, string name, string value)
        {
            var match = declared.FirstOrDefault(a => a.Key.LocalName.ToLower() == name.ToLower());
            if (match.Key == null)
            {
                transformer.SetParameter(new QName(name), new XdmAtomicValue(value));
                return;
            }
            XdmItemType type = match.Value.getDeclaredItemType();
            var typeName = "";
            if (type != null && type is XdmAtomicType)
                typeName = ((XdmAtomicType)type).Name.LocalName;
            switch (typeName.ToLower())
            {
                case "boolean":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(Convert.ToBoolean(value)));
                    break;
                case "integer":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(Convert.ToInt32(value)));
                    break;
                case "decimal":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(Convert.ToDecimal(value)));
                    break;
                case "float":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(Convert.ToSingle(value)));
                    break;
                case "double":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(Convert.ToDouble(value)));
                    break;
                case "anyuri":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(new Uri(value)));
                    break;
                case "qname":
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(new QName(value)));
                    break;
                default:
                    transformer.SetParameter(new QName(name), new XdmAtomicValue(value));
                    break;
            }
        }
    }

    /// <summary>Other stylesheets on XslCompiledTransform, as the host runs them.</summary>
    internal sealed class DotnetTransform : ITransform
    {
        private readonly XslCompiledTransform _transform = new XslCompiledTransform();

        public DotnetTransform(string stylesheet)
        {
            var settings = new XsltSettings(true, true);
            using (var s = new FileStream(stylesheet, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
            using (var reader = XmlReader.Create(s, new XmlReaderSettings { DtdProcessing = DtdProcessing.Parse }, stylesheet))
                _transform.Load(reader, settings, new XmlUrlResolver());
        }

        public string Run(string inputPath, List<KeyValuePair<string, string>> parameters, string profilePath)
        {
            var args = new XsltArgumentList();
            foreach (var arg in parameters)
                args.AddParam(arg.Key, "", arg.Value);
            // The host passes its XDocument as a string, so there is no base URI.
            var xml = XDocument.Load(inputPath).ToString();
            var fo = new StringBuilder();
            using (var stringReader = new StringReader(xml))
            using (var xmlReader = XmlReader.Create(stringReader, new XmlReaderSettings { DtdProcessing = DtdProcessing.Parse }))
            using (var writer = XmlWriter.Create(fo, new XmlWriterSettings { OmitXmlDeclaration = true }))
                _transform.Transform(xmlReader, args, writer);
            return fo.ToString();
        }
    }

    /// <summary>
    /// Times templates and functions. Saxon calls enter/leave around every
    /// traced instruction; only templates and functions are recorded. A
    /// template's own time excludes the templates and functions it called;
    /// its total time counts recursive calls once.
    /// </summary>
    internal sealed class TimingListener : net.sf.saxon.lib.TraceListener
    {
        private const int XslTemplate = 200;
        private const int XslFunction = 158;
        private const int LocationTemplate = 2008;
        private const int LocationFunction = 2065;

        private sealed class Stat
        {
            public string Kind, Name, Match, Mode, File;
            public int Line;
            public long Calls, SelfTicks, TotalTicks;
            public int Active;
        }

        private sealed class Frame
        {
            public Stat Stat;
            public long Start, ChildTicks;
        }

        private readonly Dictionary<string, Stat> _stats = new Dictionary<string, Stat>();
        private readonly Stack<Frame> _stack = new Stack<Frame>();
        private readonly long _started = Stopwatch.GetTimestamp();
        private long _finished;

        private static bool Wanted(net.sf.saxon.trace.InstructionInfo info)
        {
            int t = info.getConstructType();
            return t == XslTemplate || t == XslFunction || t == LocationTemplate || t == LocationFunction;
        }

        private static string Text(object o)
        {
            return o == null ? null : o.ToString();
        }

        public void enter(net.sf.saxon.trace.InstructionInfo info, net.sf.saxon.expr.XPathContext context)
        {
            if (!Wanted(info))
                return;
            string file = info.getSystemId();
            int line = info.getLineNumber();
            var name = info.getObjectName();
            string key = file + "#" + line + "#" + info.getColumnNumber() + "#" + (name == null ? "" : name.getClarkName());
            Stat stat;
            if (!_stats.TryGetValue(key, out stat))
            {
                int t = info.getConstructType();
                stat = new Stat
                {
                    Kind = t == XslFunction || t == LocationFunction ? "function" : "template",
                    Name = name == null ? null : name.getDisplayName(),
                    Match = Text(info.getProperty("match")),
                    Mode = Text(info.getProperty("mode")),
                    File = file,
                    Line = line,
                };
                _stats[key] = stat;
            }
            stat.Calls++;
            stat.Active++;
            _stack.Push(new Frame { Stat = stat, Start = Stopwatch.GetTimestamp() });
        }

        public void leave(net.sf.saxon.trace.InstructionInfo info)
        {
            if (!Wanted(info) || _stack.Count == 0)
                return;
            var frame = _stack.Pop();
            long elapsed = Stopwatch.GetTimestamp() - frame.Start;
            frame.Stat.SelfTicks += elapsed - frame.ChildTicks;
            frame.Stat.Active--;
            if (frame.Stat.Active == 0)
                frame.Stat.TotalTicks += elapsed;
            if (_stack.Count > 0)
                _stack.Peek().ChildTicks += elapsed;
        }

        public void open(net.sf.saxon.Controller controller) { }
        public void close() { _finished = Stopwatch.GetTimestamp(); }
        public void setOutputDestination(net.sf.saxon.lib.Logger logger) { }
        public void startCurrentItem(net.sf.saxon.om.Item item) { }
        public void endCurrentItem(net.sf.saxon.om.Item item) { }

        private static string Ms(long ticks)
        {
            return (ticks * 1000.0 / Stopwatch.Frequency).ToString("0.###", System.Globalization.CultureInfo.InvariantCulture);
        }

        public string ToJson()
        {
            long end = _finished != 0 ? _finished : Stopwatch.GetTimestamp();
            var sb = new StringBuilder();
            sb.Append("{\"total_ms\":").Append(Ms(end - _started)).Append(",\"templates\":[");
            bool first = true;
            foreach (var s in _stats.Values.OrderByDescending(x => x.SelfTicks))
            {
                if (!first)
                    sb.Append(',');
                first = false;
                sb.Append("{\"kind\":").Append(Program.Json(s.Kind));
                if (s.Name != null) sb.Append(",\"name\":").Append(Program.Json(s.Name));
                if (s.Match != null) sb.Append(",\"match\":").Append(Program.Json(s.Match));
                if (s.Mode != null) sb.Append(",\"mode\":").Append(Program.Json(s.Mode));
                sb.Append(",\"file\":").Append(Program.Json(s.File ?? ""))
                  .Append(",\"line\":").Append(s.Line)
                  .Append(",\"calls\":").Append(s.Calls)
                  .Append(",\"self_ms\":").Append(Ms(s.SelfTicks))
                  .Append(",\"total_ms\":").Append(Ms(s.TotalTicks))
                  .Append('}');
            }
            sb.Append("]}");
            return sb.ToString();
        }
    }
}
