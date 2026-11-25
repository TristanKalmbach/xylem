// Runs XSLT stylesheets on Xylem's built-in engine through its C library
// (xylem_xslt.dll), called from .NET the way a .NET publishing service calls
// Saxon-HE or XslCompiledTransform today. It takes the same command line as
// tools/saxon-dotnet's SaxonDotnetHost.exe and prints the same JSON lines, so
// xylem-xslt-check can drive either one and compare them.
//
// What it copies from such a host (see tools/saxon-dotnet/SaxonDotnetHost.cs):
//   - The stylesheet is compiled once from its file, so imports, includes and
//     document() resolve next to it; the input's base URI is its folder.
//   - XSLT 2.0: the input file's bytes are handed over as they are (the host
//     saves its XDocument to a stream and hands Saxon those bytes).
//   - XSLT 1.0: the input is read into an XDocument and handed over as its
//     string, as the host does for XslCompiledTransform; with --serialize
//     the file's text is handed over as it is.
//   - --serialize: a second step on text the host already has (XSL-FO to
//     HTML): the result follows xsl:output.
//   - The result is written UTF-8 with a byte order mark, as the host saves it.
//   - Parameters are passed with the names given; the engine converts a value
//     to a declared boolean or numeric type.
//
// Usage (the same as SaxonDotnetHost.exe):
//   XylemDotnetHost run --stylesheet <file.xsl> --job <input>=<output>
//                       [--job ...] [--param name=value ...] [--serialize]
//                       [--repeat N]
//   XylemDotnetHost compile --stylesheet <file.xsl> [--stylesheet ...]
//
// Checks that the library is safe to load into a long-running .NET process:
//   XylemDotnetHost selftest threads [--stylesheet X --input Y] [--param n=v]
//                                    [--threads 8] [--rounds 4]
//   XylemDotnetHost selftest memory  [--stylesheet X --input Y] [--param n=v]
//                                    [--iterations 1000]
//   XylemDotnetHost selftest errors  [--reference SaxonDotnetHost.exe]
//   XylemDotnetHost selftest all
//
// Built for .NET Framework 4 with the C# 5 compiler that ships with Windows
// (see build.cmd).

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Xml;
using System.Xml.Linq;

namespace Xylem.DotnetEmbed
{
    /// <summary>The C library's functions (include/xylem_xslt.h).</summary>
    internal static class Native
    {
        private const string Lib = "xylem_xslt";

        public const uint OutputPublish = 0;
        public const uint OutputSerialize = 1;
        public const uint NoFileAccess = 2;

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr xylem_xslt_version();

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr xylem_xslt_last_error();

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr xylem_xslt_compile(byte[] pathUtf8);

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern uint xylem_xslt_version_of(IntPtr ss);

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern int xylem_xslt_transform(IntPtr ss, byte[] input, UIntPtr inputLen,
            byte[] paramsJsonUtf8, uint flags, out IntPtr output, out UIntPtr outputLen);

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern void xylem_xslt_free_bytes(IntPtr ptr, UIntPtr len);

        [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
        public static extern void xylem_xslt_free(IntPtr ss);

        public static byte[] Utf8z(string s)
        {
            return Encoding.UTF8.GetBytes(s + "\0");
        }

        public static string FromUtf8z(IntPtr p)
        {
            if (p == IntPtr.Zero)
                return null;
            int n = 0;
            while (Marshal.ReadByte(p, n) != 0)
                n++;
            var bytes = new byte[n];
            Marshal.Copy(p, bytes, 0, n);
            return Encoding.UTF8.GetString(bytes);
        }

        /// <summary>The last error on this thread; read it straight after the failing call.</summary>
        public static string LastError()
        {
            var e = FromUtf8z(xylem_xslt_last_error());
            return string.IsNullOrEmpty(e) ? "unknown error" : e;
        }

        public static string Version()
        {
            return FromUtf8z(xylem_xslt_version());
        }
    }

    /// <summary>The engine failed; the message is the library's last error.</summary>
    internal sealed class XylemException : Exception
    {
        public readonly int Code;

        public XylemException(string message, int code) : base(message)
        {
            Code = code;
        }
    }

    /// <summary>A compiled stylesheet, as a host would wrap the C library.</summary>
    internal sealed class XylemStylesheet : IDisposable
    {
        private IntPtr _handle;

        public XylemStylesheet(string path)
        {
            _handle = Native.xylem_xslt_compile(Native.Utf8z(path));
            if (_handle == IntPtr.Zero)
                throw new XylemException(Native.LastError(), -1);
        }

        /// <summary>1 or 2: the XSLT version the stylesheet runs as.</summary>
        public uint XsltVersion
        {
            get { return Native.xylem_xslt_version_of(_handle); }
        }

        /// <summary>Transforms an XML document; returns the result as text.</summary>
        public string Transform(byte[] input, string paramsJson, uint flags)
        {
            if (_handle == IntPtr.Zero)
                throw new ObjectDisposedException("XylemStylesheet");
            IntPtr ptr;
            UIntPtr len;
            int rc = Native.xylem_xslt_transform(_handle, input, (UIntPtr)input.Length,
                paramsJson == null ? null : Native.Utf8z(paramsJson), flags, out ptr, out len);
            if (rc != 0)
                throw new XylemException(Native.LastError(), rc);
            try
            {
                var bytes = new byte[checked((int)len.ToUInt64())];
                if (bytes.Length > 0)
                    Marshal.Copy(ptr, bytes, 0, bytes.Length);
                return Encoding.UTF8.GetString(bytes);
            }
            finally
            {
                Native.xylem_xslt_free_bytes(ptr, len);
            }
        }

        public void Dispose()
        {
            if (_handle != IntPtr.Zero)
            {
                Native.xylem_xslt_free(_handle);
                _handle = IntPtr.Zero;
            }
        }
    }

    internal sealed class Job
    {
        public string Input;
        public string Output;
    }

    internal sealed class Options
    {
        public string Stylesheet;
        public bool CompileOnly;
        public bool Serialize;
        public bool Profile;
        public int Repeat = 1;
        public readonly List<string> Stylesheets = new List<string>();
        public readonly List<Job> Jobs = new List<Job>();
        public readonly List<KeyValuePair<string, string>> Params = new List<KeyValuePair<string, string>>();
    }

    internal static class Program
    {
        private const string Usage =
            "Usage: XylemDotnetHost run --stylesheet <file.xsl> --job <input.xml>=<output> [--job ...] [--param name=value ...] [--serialize] [--repeat N]\n" +
            "       XylemDotnetHost compile --stylesheet <file.xsl> [--stylesheet ...]\n" +
            "       XylemDotnetHost selftest threads|memory|errors|all [options]";

        private static int Main(string[] args)
        {
            try
            {
                if (args.Length > 0 && args[0] == "selftest")
                    return SelfTest.Run(args.Skip(1).ToArray());
                if (args.Length > 0 && args[0] == "selftest-case")
                    return SelfTest.RunCase(args.Skip(1).ToArray());
                Options options;
                try
                {
                    options = ParseArgs(args);
                }
                catch (ArgumentException e)
                {
                    Console.Error.WriteLine(e.Message);
                    Console.Error.WriteLine(Usage);
                    return 2;
                }
                return options.CompileOnly ? CompileAll(options.Stylesheets) : Run(options);
            }
            catch (DllNotFoundException e)
            {
                Console.Error.WriteLine("xylem_xslt.dll was not found next to the program (" + e.Message + "). Run build.cmd.");
                return 1;
            }
            catch (BadImageFormatException e)
            {
                Console.Error.WriteLine("xylem_xslt.dll does not match this process (" + (IntPtr.Size * 8) + "-bit): " + e.Message);
                return 1;
            }
        }

        private static int Run(Options options)
        {
            var stylesheet = Path.GetFullPath(options.Stylesheet);
            if (options.Profile)
                Console.Error.WriteLine("--profile: template timings are not available through the C library; running without them.");
            var compileClock = Stopwatch.StartNew();
            XylemStylesheet ss;
            try
            {
                ss = new XylemStylesheet(stylesheet);
            }
            catch (XylemException e)
            {
                Console.Error.WriteLine("The stylesheet did not compile: " + e.Message);
                Console.WriteLine("{\"summary\":true,\"xslt\":" + Json(RootVersionIs1(stylesheet) ? "1.0" : "2.0")
                    + ",\"compiled\":false,\"error\":" + Json(e.Message) + "}");
                return 1;
            }
            compileClock.Stop();

            int failed = 0;
            using (ss)
            {
                bool isXslt2 = ss.XsltVersion == 2;
                string paramsJson = ParamsJson(options.Params);
                for (int i = 0; i < options.Jobs.Count; i++)
                {
                    var job = options.Jobs[i];
                    var clock = Stopwatch.StartNew();
                    string error = null;
                    var runs = new List<long>();
                    try
                    {
                        for (int r = 0; r < Math.Max(1, options.Repeat); r++)
                        {
                            var runClock = Stopwatch.StartNew();
                            var result = HostTransform(ss, isXslt2, Path.GetFullPath(job.Input), paramsJson, options.Serialize);
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
                        error = e.Message;
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
                    if (error != null)
                        line.Append(",\"error\":").Append(Json(error));
                    line.Append('}');
                    Console.WriteLine(line.ToString());
                }
                Console.WriteLine("{\"summary\":true,\"xslt\":" + Json(isXslt2 ? "2.0" : "1.0")
                    + ",\"compiled\":true,\"compile_ms\":" + compileClock.ElapsedMilliseconds
                    + ",\"jobs\":" + options.Jobs.Count + ",\"failed\":" + failed + "}");
            }
            return failed == 0 ? 0 : 1;
        }

        /// <summary>
        /// One transformation, fed the way the host feeds its engines: the
        /// file's bytes for XSLT 2.0 (also with --serialize); for XSLT 1.0
        /// the file's text with --serialize, the XDocument's string without.
        /// </summary>
        internal static string HostTransform(XylemStylesheet ss, bool isXslt2, string inputPath, string paramsJson, bool serialize)
        {
            byte[] input;
            if (isXslt2)
                input = File.ReadAllBytes(inputPath);
            else if (serialize)
                input = Encoding.UTF8.GetBytes(File.ReadAllText(inputPath));
            else
                input = Encoding.UTF8.GetBytes(XDocument.Load(inputPath).ToString());
            return ss.Transform(input, paramsJson, serialize ? Native.OutputSerialize : Native.OutputPublish);
        }

        /// <summary>Compiles each stylesheet as a run would.</summary>
        private static int CompileAll(List<string> stylesheets)
        {
            int failed = 0;
            foreach (var s in stylesheets)
            {
                var clock = Stopwatch.StartNew();
                string error = null;
                try
                {
                    new XylemStylesheet(Path.GetFullPath(s)).Dispose();
                }
                catch (XylemException e)
                {
                    error = e.Message;
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
                throw new ArgumentException("The first argument must be 'run', 'compile' or 'selftest'.");
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
                    case "--param":
                        options.Params.Add(ParseParam(Require(args[i], value)));
                        i++;
                        break;
                    case "--serialize":
                        options.Serialize = true;
                        break;
                    case "--profile":
                        options.Profile = true;
                        break;
                    case "--allow-scripts":
                        // The engine never runs embedded scripts; accepted so
                        // the command line matches SaxonDotnetHost's.
                        break;
                    default:
                        throw new ArgumentException("Unknown option: " + args[i]);
                }
            }
            if (options.Stylesheet == null)
                throw new ArgumentException("--stylesheet is required.");
            if (!options.CompileOnly && options.Jobs.Count == 0)
                throw new ArgumentException("At least one --job is required.");
            return options;
        }

        internal static KeyValuePair<string, string> ParseParam(string pair)
        {
            int eq = pair.IndexOf('=');
            if (eq <= 0)
                throw new ArgumentException("--param takes name=value, got: " + pair);
            return new KeyValuePair<string, string>(pair.Substring(0, eq), pair.Substring(eq + 1));
        }

        internal static string Require(string option, string value)
        {
            if (value == null)
                throw new ArgumentException(option + " needs a value.");
            return value;
        }

        /// <summary>The parameters as the JSON object the library takes, or null.</summary>
        internal static string ParamsJson(List<KeyValuePair<string, string>> parameters)
        {
            if (parameters.Count == 0)
                return null;
            var sb = new StringBuilder("{");
            for (int i = 0; i < parameters.Count; i++)
            {
                if (i > 0)
                    sb.Append(',');
                sb.Append(Json(parameters[i].Key)).Append(':').Append(Json(parameters[i].Value));
            }
            return sb.Append('}').ToString();
        }

        /// <summary>For the summary of a stylesheet that did not compile.</summary>
        private static bool RootVersionIs1(string stylesheet)
        {
            try
            {
                var settings = new XmlReaderSettings { DtdProcessing = DtdProcessing.Parse };
                using (var reader = XmlReader.Create(stylesheet, settings))
                {
                    reader.MoveToContent();
                    var v = reader.GetAttribute("version");
                    return v != null && v.StartsWith("1");
                }
            }
            catch (Exception)
            {
                return false;
            }
        }

        internal static long Median(List<long> values)
        {
            var sorted = values.OrderBy(v => v).ToList();
            return sorted[sorted.Count / 2];
        }

        internal static string Json(string s)
        {
            if (s == null)
                return "null";
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

    /// <summary>
    /// Checks that the library is safe inside a long-running .NET process:
    /// one compiled stylesheet shared by many threads, no memory growth over
    /// many compile/transform/free cycles, and errors (including endless
    /// recursion and missing documents) that come back as exceptions rather
    /// than taking the process down. Each check prints JSON lines and exits
    /// 0 only when it passed (known engine problems count as expected
    /// failures and are labelled "xfail").
    /// </summary>
    internal static class SelfTest
    {
        private sealed class Args
        {
            public string Stylesheet;
            public string Input;
            public string Reference;
            public int Threads = 8;
            public int Rounds = 4;
            public int Iterations = 1000;
            /// <summary>memory: null (compile, transform, free), "compile" or "transform".</summary>
            public string Only;
            public readonly List<KeyValuePair<string, string>> Params = new List<KeyValuePair<string, string>>();
        }

        public static int Run(string[] argv)
        {
            if (argv.Length == 0)
            {
                Console.Error.WriteLine("Usage: XylemDotnetHost selftest threads|memory|errors|all [--stylesheet X --input Y] [--param n=v] [--threads N] [--rounds N] [--iterations N] [--reference SaxonDotnetHost.exe]");
                return 2;
            }
            Args a;
            try
            {
                a = Parse(argv.Skip(1).ToArray());
            }
            catch (ArgumentException e)
            {
                Console.Error.WriteLine(e.Message);
                return 2;
            }
            Console.WriteLine("{\"selftest\":\"process\",\"library\":" + Program.Json(Native.Version())
                + ",\"bits\":" + (IntPtr.Size * 8) + ",\"clr\":" + Program.Json(Environment.Version.ToString()) + "}");
            string scratch = Path.Combine(Path.GetTempPath(), "xylem-dotnet-embed-" + Process.GetCurrentProcess().Id);
            Directory.CreateDirectory(scratch);
            try
            {
                switch (argv[0])
                {
                    case "threads": return Threads(a, scratch) ? 0 : 1;
                    case "memory": return Memory(a, scratch) ? 0 : 1;
                    case "errors": return Errors(a, scratch) ? 0 : 1;
                    case "all":
                        {
                            bool ok = Threads(a, scratch);
                            ok &= Memory(a, scratch);
                            ok &= Errors(a, scratch);
                            return ok ? 0 : 1;
                        }
                    default:
                        Console.Error.WriteLine("Unknown self-test: " + argv[0]);
                        return 2;
                }
            }
            finally
            {
                try { Directory.Delete(scratch, true); } catch (Exception) { }
            }
        }

        private static Args Parse(string[] argv)
        {
            var a = new Args();
            for (int i = 0; i < argv.Length; i++)
            {
                string value = i + 1 < argv.Length ? argv[i + 1] : null;
                switch (argv[i])
                {
                    case "--stylesheet": a.Stylesheet = Path.GetFullPath(Program.Require(argv[i], value)); i++; break;
                    case "--input": a.Input = Path.GetFullPath(Program.Require(argv[i], value)); i++; break;
                    case "--reference": a.Reference = Path.GetFullPath(Program.Require(argv[i], value)); i++; break;
                    case "--param": a.Params.Add(Program.ParseParam(Program.Require(argv[i], value))); i++; break;
                    case "--threads": a.Threads = PositiveInt(argv[i], value); i++; break;
                    case "--rounds": a.Rounds = PositiveInt(argv[i], value); i++; break;
                    case "--iterations": a.Iterations = PositiveInt(argv[i], value); i++; break;
                    case "--only":
                        a.Only = Program.Require(argv[i], value);
                        if (a.Only != "compile" && a.Only != "transform")
                            throw new ArgumentException("--only takes compile or transform.");
                        i++;
                        break;
                    default: throw new ArgumentException("Unknown option: " + argv[i]);
                }
            }
            if ((a.Stylesheet == null) != (a.Input == null))
                throw new ArgumentException("--stylesheet and --input go together.");
            return a;
        }

        private static int PositiveInt(string option, string value)
        {
            int n;
            if (!int.TryParse(Program.Require(option, value), out n) || n < 1)
                throw new ArgumentException(option + " takes a positive number.");
            return n;
        }

        // ------------------------------------------------------------------
        // A stylesheet and input of our own, for when none is given. It uses
        // the parts of the engine a publishing stylesheet leans on: keys,
        // sorting, numbering, functions, generate-id(), document() of an
        // included file, and a global parameter.

        private const string SampleStylesheet =
@"<xsl:stylesheet version=""2.0"" xmlns:xsl=""http://www.w3.org/1999/XSL/Transform""
    xmlns:xs=""http://www.w3.org/2001/XMLSchema"" xmlns:f=""urn:sample"" exclude-result-prefixes=""xs f"">
  <xsl:include href=""sample-common.xsl""/>
  <xsl:param name=""mode"" select=""'draft'""/>
  <xsl:param name=""pageCount"" as=""xs:integer"" select=""1""/>
  <xsl:key name=""by-ref"" match=""item"" use=""@ref""/>
  <xsl:variable name=""labels"" select=""document('sample-labels.xml')/labels""/>
  <xsl:function name=""f:title"" as=""xs:string"">
    <xsl:param name=""s"" as=""xs:string""/>
    <xsl:sequence select=""concat(upper-case(substring($s, 1, 1)), substring($s, 2))""/>
  </xsl:function>
  <xsl:template match=""/"">
    <doc mode=""{$mode}"" pages=""{$pageCount + 1}"">
      <xsl:apply-templates select=""book/chapter"">
        <xsl:sort select=""@order"" data-type=""number""/>
      </xsl:apply-templates>
    </doc>
  </xsl:template>
  <xsl:template match=""chapter"">
    <section id=""{generate-id()}"" n=""{position()}"">
      <xsl:call-template name=""heading""/>
      <xsl:for-each select=""item"">
        <entry ref=""{@ref}"" same=""{count(key('by-ref', @ref))}"">
          <xsl:number level=""multiple"" count=""chapter|item"" format=""1.1""/>
          <xsl:text> </xsl:text>
          <xsl:value-of select=""f:title(string(.))""/>
          <xsl:value-of select=""$labels/label[@ref = current()/@ref]""/>
        </entry>
      </xsl:for-each>
    </section>
  </xsl:template>
</xsl:stylesheet>";

        private const string SampleCommon =
@"<xsl:stylesheet version=""2.0"" xmlns:xsl=""http://www.w3.org/1999/XSL/Transform"">
  <xsl:template name=""heading""><title><xsl:value-of select=""normalize-space(@title)""/></title></xsl:template>
</xsl:stylesheet>";

        private const string SampleLabels =
@"<labels><label ref=""a""> (alpha)</label><label ref=""b""> (beta)</label><label ref=""c""> (gamma)</label></labels>";

        private static string SampleInput()
        {
            var sb = new StringBuilder("<book>");
            for (int c = 0; c < 80; c++)
            {
                sb.Append("<chapter order=\"").Append((c * 37) % 80).Append("\" title=\"  Chapter ").Append(c).Append("  \">");
                for (int i = 0; i < 20; i++)
                    sb.Append("<item ref=\"").Append("abcde"[(c + i) % 5]).Append("\">entry ").Append(c).Append('.').Append(i).Append("</item>");
                sb.Append("</chapter>");
            }
            return sb.Append("</book>").ToString();
        }

        private static void WriteSample(string dir, out string stylesheet, out string input)
        {
            stylesheet = Path.Combine(dir, "sample.xsl");
            input = Path.Combine(dir, "sample.xml");
            File.WriteAllText(stylesheet, SampleStylesheet, new UTF8Encoding(false));
            File.WriteAllText(Path.Combine(dir, "sample-common.xsl"), SampleCommon, new UTF8Encoding(false));
            File.WriteAllText(Path.Combine(dir, "sample-labels.xml"), SampleLabels, new UTF8Encoding(false));
            File.WriteAllText(input, SampleInput(), new UTF8Encoding(false));
        }

        private static void Target(Args a, string scratch, out string stylesheet, out string input, out string paramsJson)
        {
            if (a.Stylesheet != null)
            {
                stylesheet = a.Stylesheet;
                input = a.Input;
                paramsJson = Program.ParamsJson(a.Params);
                return;
            }
            WriteSample(scratch, out stylesheet, out input);
            var p = new List<KeyValuePair<string, string>>(a.Params);
            if (p.Count == 0)
            {
                p.Add(new KeyValuePair<string, string>("mode", "final"));
                p.Add(new KeyValuePair<string, string>("pageCount", "41"));
            }
            paramsJson = Program.ParamsJson(p);
        }

        // ------------------------------------------------------------------
        // threads: one compiled stylesheet, used from N threads at once.

        private static bool Threads(Args a, string scratch)
        {
            string stylesheet, input, paramsJson;
            Target(a, scratch, out stylesheet, out input, out paramsJson);
            string expected;
            long singleMs;
            using (var ss = new XylemStylesheet(stylesheet))
            {
                bool isXslt2 = ss.XsltVersion == 2;
                var clock = Stopwatch.StartNew();
                expected = Program.HostTransform(ss, isXslt2, input, paramsJson, false);
                singleMs = clock.ElapsedMilliseconds;

                // Shared: every thread uses the same handle at once.
                long sharedMs;
                var shared = RunThreads(a.Threads, a.Rounds, delegate { return ss; }, false, isXslt2, input, paramsJson, expected, out sharedMs);
                Report("threads-shared-handle", shared, a, singleMs, sharedMs);

                // Own: one handle per thread, compiled on that thread.
                long ownMs;
                var own = RunThreads(a.Threads, a.Rounds, delegate { return new XylemStylesheet(stylesheet); }, true, isXslt2, input, paramsJson, expected, out ownMs);
                Report("threads-handle-per-thread", own, a, singleMs, ownMs);
                return Passed(shared) && Passed(own);
            }
        }

        private sealed class ThreadResults
        {
            public int Runs;
            public int Same;
            public int SameAfterIds;
            public int Failures;
            public readonly List<string> Errors = new List<string>();
        }

        private delegate XylemStylesheet HandleFor();

        /// <summary>
        /// Runs the transformation <paramref name="rounds"/> times on each of
        /// <paramref name="count"/> threads, started together, and compares
        /// every result with <paramref name="expected"/>. A result that
        /// differs only in generate-id() document numbers is counted apart
        /// (and still fails the check).
        /// </summary>
        private static ThreadResults RunThreads(int count, int rounds, HandleFor handleFor, bool ownHandles, bool isXslt2,
            string input, string paramsJson, string expected, out long elapsedMs)
        {
            var results = new ThreadResults();
            var gate = new Barrier(count);
            var threads = new List<Thread>();
            var expectedIds = NormaliseIds(expected);
            var clock = new Stopwatch();
            for (int t = 0; t < count; t++)
            {
                var thread = new Thread(delegate ()
                {
                    XylemStylesheet ss = null;
                    try
                    {
                        ss = handleFor();
                        if (gate.SignalAndWait(60000) && Interlocked.CompareExchange(ref _clockStarted, 1, 0) == 0)
                            clock.Start();
                        for (int r = 0; r < rounds; r++)
                        {
                            string got = Program.HostTransform(ss, isXslt2, input, paramsJson, false);
                            lock (results)
                            {
                                results.Runs++;
                                if (got == expected)
                                    results.Same++;
                                else if (NormaliseIds(got) == expectedIds)
                                    results.SameAfterIds++;
                                else
                                {
                                    results.Failures++;
                                    results.Errors.Add("output differs (" + got.Length + " characters, expected " + expected.Length + ")");
                                }
                            }
                        }
                    }
                    catch (Exception e)
                    {
                        lock (results)
                        {
                            results.Failures++;
                            results.Errors.Add(e.GetType().Name + ": " + e.Message);
                        }
                    }
                    finally
                    {
                        // A handle made for this thread is freed on it.
                        if (ss != null && ownHandles)
                            ss.Dispose();
                    }
                });
                threads.Add(thread);
            }
            _clockStarted = 0;
            foreach (var t in threads)
                t.Start();
            foreach (var t in threads)
                t.Join();
            clock.Stop();
            elapsedMs = clock.ElapsedMilliseconds;
            return results;
        }

        private static int _clockStarted;

        private static bool Passed(ThreadResults r)
        {
            return r.Failures == 0 && r.SameAfterIds == 0;
        }

        private static void Report(string name, ThreadResults r, Args a, long singleMs, long elapsedMs)
        {
            var line = new StringBuilder();
            line.Append("{\"selftest\":").Append(Program.Json(name))
                .Append(",\"threads\":").Append(a.Threads)
                .Append(",\"rounds\":").Append(a.Rounds)
                .Append(",\"runs\":").Append(r.Runs)
                .Append(",\"identical\":").Append(r.Same)
                .Append(",\"identical_after_generate_id\":").Append(r.SameAfterIds)
                .Append(",\"failed\":").Append(r.Failures)
                .Append(",\"single_run_ms\":").Append(singleMs)
                .Append(",\"all_threads_ms\":").Append(elapsedMs)
                .Append(",\"ok\":").Append(Passed(r) ? "true" : "false");
            if (r.Errors.Count > 0)
                line.Append(",\"errors\":[").Append(string.Join(",", r.Errors.Distinct().Take(5).Select(Program.Json))).Append(']');
            Console.WriteLine(line.Append('}').ToString());
        }

        private static readonly Regex IdPattern = new Regex(@"\bd(\d+)e(\d+)\b");

        /// <summary>generate-id() values with their document numbers renumbered in order of appearance.</summary>
        private static string NormaliseIds(string s)
        {
            var seen = new Dictionary<string, int>();
            return IdPattern.Replace(s, delegate (Match m)
            {
                int n;
                if (!seen.TryGetValue(m.Groups[1].Value, out n))
                {
                    n = seen.Count + 1;
                    seen[m.Groups[1].Value] = n;
                }
                return "d#" + n + "e" + m.Groups[2].Value;
            });
        }

        // ------------------------------------------------------------------
        // memory: compile, transform and free, many times over.

        private static bool Memory(Args a, string scratch)
        {
            string stylesheet, input, paramsJson;
            Target(a, scratch, out stylesheet, out input, out paramsJson);
            var samples = new List<long>();
            int every = Math.Max(1, a.Iterations / 30);
            string first = null;
            int differing = 0;
            // --only transform keeps one handle for every iteration;
            // --only compile compiles and frees without transforming.
            XylemStylesheet kept = a.Only == "transform" ? new XylemStylesheet(stylesheet) : null;
            var clock = Stopwatch.StartNew();
            for (int i = 0; i < a.Iterations; i++)
            {
                var ss = kept ?? new XylemStylesheet(stylesheet);
                try
                {
                    if (a.Only != "compile")
                    {
                        var got = Program.HostTransform(ss, ss.XsltVersion == 2, input, paramsJson, false);
                        if (first == null)
                            first = got;
                        else if (got != first)
                            differing++;
                    }
                }
                finally
                {
                    if (kept == null)
                        ss.Dispose();
                }
                if ((i + 1) % every == 0)
                    samples.Add(PrivateBytes());
            }
            clock.Stop();
            if (kept != null)
                kept.Dispose();
            // The first third warms up the allocator and the CLR. Growth is
            // judged on the peak of the middle third against the peak of the
            // last third: between samples the heap may or may not have given
            // a large input's freed tree back to the system, so single
            // samples swing, but a leak keeps raising the peak.
            int third = samples.Count / 3;
            long middle = samples.Skip(third).Take(third).Max();
            long last = samples.Skip(2 * third).Max();
            long growth = last - middle;
            // Allow 4 MB or 5 % for allocator and runtime noise.
            long allowed = Math.Max(4L << 20, middle / 20);
            bool ok = growth <= allowed && differing == 0;
            Console.WriteLine("{\"selftest\":\"memory\",\"cycle\":" + Program.Json(a.Only ?? "compile+transform+free") + ",\"iterations\":" + a.Iterations
                + ",\"ms\":" + clock.ElapsedMilliseconds
                + ",\"private_mb_first\":" + Mb(samples.First())
                + ",\"private_mb_peak_middle_third\":" + Mb(middle)
                + ",\"private_mb_peak_last_third\":" + Mb(last)
                + ",\"growth_mb\":" + Mb(growth)
                + ",\"allowed_mb\":" + Mb(allowed)
                + ",\"outputs_differing\":" + differing
                + ",\"samples_mb\":[" + string.Join(",", samples.Select(Mb)) + "]"
                + ",\"ok\":" + (ok ? "true" : "false") + "}");
            return ok;
        }

        private static long PrivateBytes()
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
            using (var p = Process.GetCurrentProcess())
            {
                p.Refresh();
                return p.PrivateMemorySize64;
            }
        }

        private static string Mb(long bytes)
        {
            return (bytes / 1048576.0).ToString("0.0", System.Globalization.CultureInfo.InvariantCulture);
        }

        // ------------------------------------------------------------------
        // errors: each case runs in a child process (this program again), so
        // a case that crashes shows up as that child's exit code instead of
        // ending the test.

        private sealed class Case
        {
            public string Name;
            public string What;
            public string Stylesheet;
            public string Input = "<in/>";
            /// <summary>"compile" or "transform": where the error must come back.</summary>
            public string ErrorAt = "transform";
        }

        private const string Xsl2 = "<xsl:stylesheet version=\"2.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\" xmlns:f=\"urn:f\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">";

        private static List<Case> Cases()
        {
            return new List<Case>
            {
                new Case
                {
                    Name = "message-terminate",
                    What = "xsl:message terminate=\"yes\"",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><xsl:message terminate=\"yes\">stopped on purpose</xsl:message></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "error-function",
                    What = "fn:error() raised by the stylesheet",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r><xsl:value-of select=\"error(QName('urn:f', 'f:boom'), 'raised on purpose')\"/></r></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "type-error",
                    What = "a dynamic type error (xs:integer('x'))",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r><xsl:value-of select=\"xs:integer('x') + 1\"/></r></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "recursion-template",
                    What = "a named template that calls itself without end",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><xsl:call-template name=\"down\"><xsl:with-param name=\"n\" select=\"1\"/></xsl:call-template></xsl:template>"
                        + "<xsl:template name=\"down\"><xsl:param name=\"n\"/><level n=\"{$n}\"><xsl:call-template name=\"down\"><xsl:with-param name=\"n\" select=\"$n + 1\"/></xsl:call-template></level></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "recursion-function",
                    What = "an xsl:function that calls itself without end",
                    Stylesheet = Xsl2 + "<xsl:function name=\"f:down\" as=\"xs:integer\"><xsl:param name=\"n\" as=\"xs:integer\"/><xsl:sequence select=\"f:down($n + 1) + 1\"/></xsl:function>"
                        + "<xsl:template match=\"/\"><r><xsl:value-of select=\"f:down(1)\"/></r></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "recursion-apply-templates",
                    What = "a template rule that applies templates to the same node without end",
                    Stylesheet = Xsl2 + "<xsl:template match=\"in\"><x><xsl:apply-templates select=\".\"/></x></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "document-missing",
                    What = "document() of a file that does not exist",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r><xsl:copy-of select=\"document('does-not-exist.xml')\"/></r></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "doc-missing",
                    What = "fn:doc() of a file that does not exist",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r><xsl:copy-of select=\"doc('does-not-exist.xml')\"/></r></xsl:template></xsl:stylesheet>",
                },
                new Case
                {
                    Name = "compile-error",
                    What = "a stylesheet whose XPath does not parse",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r><xsl:value-of select=\"1 +\"/></r></xsl:template></xsl:stylesheet>",
                    ErrorAt = "compile",
                },
                new Case
                {
                    Name = "input-not-xml",
                    What = "an input that is not well-formed",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r/></xsl:template></xsl:stylesheet>",
                    Input = "<in><unclosed></in>",
                },
                new Case
                {
                    Name = "input-nested-2000",
                    What = "an input nested 2000 elements deep",
                    Stylesheet = Xsl2 + "<xsl:template match=\"/\"><r/></xsl:template></xsl:stylesheet>",
                    Input = DeepInput(2000),
                },
            };
        }

        private static string DeepInput(int depth)
        {
            var sb = new StringBuilder(depth * 8);
            for (int i = 0; i < depth; i++)
                sb.Append("<e>");
            for (int i = 0; i < depth; i++)
                sb.Append("</e>");
            return sb.ToString();
        }

        /// <summary>Engine problems known at the time of writing; see README.md.</summary>
        private static readonly Dictionary<string, string> KnownIssues = new Dictionary<string, string>
        {
            { "recursion-template", "no recursion limit: the native stack overflows and the process ends (0xC00000FD); Saxon on .NET ends the same way" },
            { "recursion-function", "no recursion limit: the native stack overflows and the process ends (0xC00000FD); Saxon on .NET ends the same way" },
            { "recursion-apply-templates", "no recursion limit: the native stack overflows and the process ends (0xC00000FD); Saxon on .NET ends the same way" },
            { "document-missing", "document() of a missing file gives an empty node-set without an error; Saxon does the same (a recoverable error in XSLT 2.0)" },
            { "doc-missing", "fn:doc() of a missing file gives an empty sequence; it must raise FODC0002, as Saxon does" },
            { "input-nested-2000", "the input parser or tree recurses per element: about 1,150 levels overflow a 1 MB stack and end the process (0xC00000FD); Saxon reads it" },
        };

        private static bool Errors(Args a, string scratch)
        {
            bool allOk = true;
            string exe = Process.GetCurrentProcess().MainModule.FileName;
            foreach (var c in Cases())
            {
                string dir = Path.Combine(scratch, c.Name);
                Directory.CreateDirectory(dir);
                string xsl = Path.Combine(dir, c.Name + ".xsl");
                string xml = Path.Combine(dir, c.Name + ".xml");
                File.WriteAllText(xsl, c.Stylesheet, new UTF8Encoding(false));
                File.WriteAllText(xml, c.Input, new UTF8Encoding(false));

                string childOut;
                int exitCode;
                bool timedOut;
                long ms;
                RunChild(exe, "selftest-case " + Quote(c.ErrorAt) + " " + Quote(xsl) + " " + Quote(xml), 120000,
                    out childOut, out exitCode, out timedOut, out ms);
                string reported = childOut.Split('\n').Select(l => l.Trim()).LastOrDefault(l => l.StartsWith("{")) ?? "";
                string result;
                if (timedOut)
                    result = "hang";
                else if (exitCode == 0 && reported.Contains("\"error_returned\":true"))
                    result = "error";
                else if (exitCode == 0)
                    result = "no-error";
                else if (exitCode == 3)
                    result = "wrong-place";
                else
                    result = "crash";
                bool passed = result == "error";
                string known;
                KnownIssues.TryGetValue(c.Name, out known);
                string verdict = passed ? (known != null ? "xpass" : "pass") : (known != null ? "xfail" : "fail");
                if (verdict == "fail")
                    allOk = false;

                string reference = null;
                if (a.Reference != null)
                    reference = RunReference(a.Reference, xsl, xml, Path.Combine(dir, "reference.out"));

                var line = new StringBuilder();
                line.Append("{\"selftest\":\"error\",\"case\":").Append(Program.Json(c.Name))
                    .Append(",\"what\":").Append(Program.Json(c.What))
                    .Append(",\"result\":").Append(Program.Json(result))
                    .Append(",\"verdict\":").Append(Program.Json(verdict))
                    .Append(",\"child_exit\":").Append(Program.Json(timedOut ? "timeout" : "0x" + exitCode.ToString("X8")))
                    .Append(",\"ms\":").Append(ms);
                if (reported.Length > 0)
                    line.Append(",\"child\":").Append(reported);
                if (known != null)
                    line.Append(",\"known_issue\":").Append(Program.Json(known));
                if (reference != null)
                    line.Append(",\"reference\":").Append(Program.Json(reference));
                Console.WriteLine(line.Append('}').ToString());
            }
            return allOk;
        }

        /// <summary>How the reference program (SaxonDotnetHost.exe) handled the same case.</summary>
        private static string RunReference(string program, string xsl, string xml, string output)
        {
            string text;
            int exitCode;
            bool timedOut;
            long ms;
            RunChild(program, "run --stylesheet " + Quote(xsl) + " --job " + Quote(xml + "=" + output), 120000,
                out text, out exitCode, out timedOut, out ms);
            if (timedOut)
                return "hang";
            var job = text.Split('\n').Select(l => l.Trim()).FirstOrDefault(l => l.StartsWith("{\"job\""));
            var summary = text.Split('\n').Select(l => l.Trim()).FirstOrDefault(l => l.StartsWith("{\"summary\""));
            if (summary != null && summary.Contains("\"compiled\":false"))
                return "compile error";
            if (job == null)
                return "crash (exit 0x" + exitCode.ToString("X8") + ")";
            if (job.Contains("\"ok\":true"))
            {
                string got = File.Exists(output) ? File.ReadAllText(output) : "";
                return "no error, output " + (got.Length > 80 ? got.Substring(0, 80) + "..." : got);
            }
            return "error";
        }

        private static void RunChild(string exe, string arguments, int timeoutMs,
            out string stdout, out int exitCode, out bool timedOut, out long ms)
        {
            var info = new ProcessStartInfo(exe, arguments)
            {
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
            };
            var clock = Stopwatch.StartNew();
            using (var p = Process.Start(info))
            {
                var output = new StringBuilder();
                p.OutputDataReceived += delegate (object s, DataReceivedEventArgs e) { if (e.Data != null) lock (output) output.AppendLine(e.Data); };
                p.ErrorDataReceived += delegate (object s, DataReceivedEventArgs e) { };
                p.BeginOutputReadLine();
                p.BeginErrorReadLine();
                timedOut = !p.WaitForExit(timeoutMs);
                if (timedOut)
                {
                    try { p.Kill(); } catch (Exception) { }
                    p.WaitForExit();
                    exitCode = -1;
                }
                else
                {
                    p.WaitForExit();
                    exitCode = p.ExitCode;
                }
                clock.Stop();
                ms = clock.ElapsedMilliseconds;
                lock (output)
                    stdout = output.ToString();
            }
        }

        private static string Quote(string s)
        {
            return "\"" + s.Replace("\"", "\\\"") + "\"";
        }

        [DllImport("kernel32.dll")]
        private static extern uint SetErrorMode(uint mode);

        /// <summary>
        /// The child side of the errors check: compiles and runs one case on
        /// a thread with the default stack, as a host's worker thread would,
        /// and prints what came back. Exit code 0 means the process survived;
        /// 3 means the error came back at the wrong step.
        /// </summary>
        public static int RunCase(string[] argv)
        {
            // No crash dialog: a crash should end the child, not wait for a click.
            const uint SemFailCriticalErrors = 0x0001, SemNoGpFaultErrorBox = 0x0002;
            SetErrorMode(SemFailCriticalErrors | SemNoGpFaultErrorBox);
            string errorAt = argv[0], xsl = argv[1], xml = argv[2];
            string line = null;
            int code = 0;
            var worker = new Thread(delegate ()
            {
                var clock = Stopwatch.StartNew();
                XylemStylesheet ss = null;
                try
                {
                    try
                    {
                        ss = new XylemStylesheet(xsl);
                    }
                    catch (XylemException e)
                    {
                        code = errorAt == "compile" ? 0 : 3;
                        line = Line("compile", true, e.Message, e.Code, clock.ElapsedMilliseconds);
                        return;
                    }
                    try
                    {
                        string got = Program.HostTransform(ss, ss.XsltVersion == 2, xml, null, false);
                        code = 0;
                        line = Line("transform", false, got.Length > 200 ? got.Substring(0, 200) + "..." : got, 0, clock.ElapsedMilliseconds);
                    }
                    catch (XylemException e)
                    {
                        code = errorAt == "transform" ? 0 : 3;
                        line = Line("transform", true, e.Message, e.Code, clock.ElapsedMilliseconds);
                    }
                }
                finally
                {
                    if (ss != null)
                        ss.Dispose();
                }
            });
            worker.Start();
            worker.Join();
            Console.WriteLine(line);
            return code;
        }

        private static string Line(string step, bool error, string text, int rc, long ms)
        {
            if (text != null && text.Length > 300)
                text = text.Substring(0, 300) + "...";
            return "{\"step\":" + Program.Json(step) + ",\"error_returned\":" + (error ? "true" : "false")
                + ",\"rc\":" + rc + ",\"ms\":" + ms + (error ? ",\"error\":" : ",\"output\":") + Program.Json(text) + "}";
        }
    }
}
