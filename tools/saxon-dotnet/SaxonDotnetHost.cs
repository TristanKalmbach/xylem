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
//                       [--job ...] [--param name=value ...]
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
                    ? (ITransform)new SaxonTransform(stylesheet)
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
                try
                {
                    var result = transform.Run(Path.GetFullPath(job.Input), options.Params);
                    var outputDir = Path.GetDirectoryName(Path.GetFullPath(job.Output));
                    if (!string.IsNullOrEmpty(outputDir))
                        Directory.CreateDirectory(outputDir);
                    // As the host writes it: UTF-8 with a byte order mark.
                    File.WriteAllText(job.Output, result, Encoding.UTF8);
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
                    .Append(",\"ms\":").Append(clock.ElapsedMilliseconds);
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

        private static Options ParseArgs(string[] args)
        {
            if (args.Length == 0 || args[0] != "run")
                throw new ArgumentException("The first argument must be 'run'.");
            var options = new Options();
            for (int i = 1; i < args.Length; i++)
            {
                string value = i + 1 < args.Length ? args[i + 1] : null;
                switch (args[i])
                {
                    case "--stylesheet":
                        options.Stylesheet = Require(args[i], value);
                        i++;
                        break;
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

        private static string Json(string s)
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
        string Run(string inputPath, List<KeyValuePair<string, string>> parameters);
    }

    /// <summary>XSLT 2.0 on Saxon-HE, as the host runs it.</summary>
    internal sealed class SaxonTransform : ITransform
    {
        private readonly string _stylesheet;
        private readonly XsltExecutable _executable;

        public SaxonTransform(string stylesheet)
        {
            _stylesheet = stylesheet;
            var processor = new Processor();
            var compiler = processor.NewXsltCompiler();
            _executable = compiler.Compile(new Uri(stylesheet));
        }

        public string Run(string inputPath, List<KeyValuePair<string, string>> parameters)
        {
            var declared = _executable.GetGlobalParameters();
            var destination = new DomDestination();
            // The host saves its XDocument to a stream and hands Saxon those
            // bytes; the saved input file holds the same bytes.
            using (var input = new FileStream(inputPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
            {
                var transformer = _executable.Load();
                var stylesheetFolder = new FileInfo(_stylesheet).DirectoryName;
                transformer.SetInputStream(input, new Uri(stylesheetFolder));
                foreach (var arg in parameters)
                    SetParameter(transformer, declared, arg.Key, arg.Value);
                transformer.Run(destination);
            }
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

        public string Run(string inputPath, List<KeyValuePair<string, string>> parameters)
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
}
