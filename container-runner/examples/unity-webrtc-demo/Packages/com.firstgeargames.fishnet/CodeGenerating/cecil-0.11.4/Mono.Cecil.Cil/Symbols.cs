//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.Cil;
using MonoFN.Cecil.PE;
using MonoFN.Collections.Generic;
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using MonoFN.Cecil.Metadata;
using SR = System.Reflection;

namespace MonoFN.Cecil.Cil
{
    [StructLayout(LayoutKind.Sequential)]
    public struct ImageDebugDirectory
    {
        public const int Size = 28;
        public int Characteristics;
        public int TimeDateStamp;
        public short MajorVersion;
        public short MinorVersion;
        public ImageDebugType Type;
        public int SizeOfData;
        public int AddressOfRawData;
        public int PointerToRawData;
    }

    public enum ImageDebugType
    {
        CodeView = 2,
        Deterministic = 16,
        EmbeddedPortablePdb = 17
    }

    public sealed class ImageDebugHeader
    {
        public bool HasEntries
        {
            get { return !Entries.IsNullOrEmpty(); }
        }
        public ImageDebugHeaderEntry[] Entries { get; }

        public ImageDebugHeader(ImageDebugHeaderEntry[] entries)
        {
            this.Entries = entries ?? Empty<ImageDebugHeaderEntry>.Array;
        }

        public ImageDebugHeader() : this(Empty<ImageDebugHeaderEntry>.Array) { }
        public ImageDebugHeader(ImageDebugHeaderEntry entry) : this(new[] { entry }) { }
    }

    public sealed class ImageDebugHeaderEntry
    {
        public ImageDebugDirectory Directory { get; internal set; }
        public byte[] Data { get; }

        public ImageDebugHeaderEntry(ImageDebugDirectory directory, byte[] data)
        {
            this.Directory = directory;
            this.Data = data ?? Empty<byte>.Array;
        }
    }

    public sealed class ScopeDebugInformation : DebugInformation
    {
        internal InstructionOffset start;
        internal InstructionOffset end;
        internal ImportDebugInformation import;
        internal Collection<ScopeDebugInformation> scopes;
        internal Collection<VariableDebugInformation> variables;
        internal Collection<ConstantDebugInformation> constants;
        public InstructionOffset Start
        {
            get { return start; }
            set { start = value; }
        }
        public InstructionOffset End
        {
            get { return end; }
            set { end = value; }
        }
        public ImportDebugInformation Import
        {
            get { return import; }
            set { import = value; }
        }
        public bool HasScopes
        {
            get { return !scopes.IsNullOrEmpty(); }
        }
        public Collection<ScopeDebugInformation> Scopes
        {
            get
            {
                if (scopes == null)
                    Interlocked.CompareExchange(ref scopes, new(), null);

                return scopes;
            }
        }
        public bool HasVariables
        {
            get { return !variables.IsNullOrEmpty(); }
        }
        public Collection<VariableDebugInformation> Variables
        {
            get
            {
                if (variables == null)
                    Interlocked.CompareExchange(ref variables, new(), null);

                return variables;
            }
        }
        public bool HasConstants
        {
            get { return !constants.IsNullOrEmpty(); }
        }
        public Collection<ConstantDebugInformation> Constants
        {
            get
            {
                if (constants == null)
                    Interlocked.CompareExchange(ref constants, new(), null);

                return constants;
            }
        }

        internal ScopeDebugInformation()
        {
            token = new(TokenType.LocalScope);
        }

        public ScopeDebugInformation(Instruction start, Instruction end) : this()
        {
            if (start == null)
                throw new ArgumentNullException("start");

            this.start = new(start);

            if (end != null)
                this.end = new(end);
        }

        public bool TryGetName(VariableDefinition variable, out string name)
        {
            name = null;
            if (variables == null || variables.Count == 0)
                return false;

            for (int i = 0; i < variables.Count; i++)
            {
                if (variables[i].Index == variable.Index)
                {
                    name = variables[i].Name;
                    return true;
                }
            }

            return false;
        }
    }

    public struct InstructionOffset
    {
        private readonly int? offset;
        public int Offset
        {
            get
            {
                if (ResolvedInstruction != null)
                    return ResolvedInstruction.Offset;
                if (offset.HasValue)
                    return offset.Value;

                throw new NotSupportedException();
            }
        }
        public bool IsEndOfMethod
        {
            get { return ResolvedInstruction == null && !offset.HasValue; }
        }
        internal bool IsResolved => ResolvedInstruction != null || !offset.HasValue;
        internal Instruction ResolvedInstruction { get; }

        public InstructionOffset(Instruction instruction)
        {
            if (instruction == null)
                throw new ArgumentNullException("instruction");

            this.ResolvedInstruction = instruction;
            offset = null;
        }

        public InstructionOffset(int offset)
        {
            ResolvedInstruction = null;
            this.offset = offset;
        }
    }

    [Flags]
    public enum VariableAttributes : ushort
    {
        None = 0,
        DebuggerHidden = 1
    }

    public struct VariableIndex
    {
        private readonly int? index;
        public int Index
        {
            get
            {
                if (ResolvedVariable != null)
                    return ResolvedVariable.Index;
                if (index.HasValue)
                    return index.Value;

                throw new NotSupportedException();
            }
        }
        internal bool IsResolved => ResolvedVariable != null;
        internal VariableDefinition ResolvedVariable { get; }

        public VariableIndex(VariableDefinition variable)
        {
            if (variable == null)
                throw new ArgumentNullException("variable");

            this.ResolvedVariable = variable;
            index = null;
        }

        public VariableIndex(int index)
        {
            ResolvedVariable = null;
            this.index = index;
        }
    }

    public abstract class DebugInformation : ICustomDebugInformationProvider
    {
        internal MetadataToken token;
        internal Collection<CustomDebugInformation> custom_infos;
        public MetadataToken MetadataToken
        {
            get { return token; }
            set { token = value; }
        }
        public bool HasCustomDebugInformations
        {
            get { return !custom_infos.IsNullOrEmpty(); }
        }
        public Collection<CustomDebugInformation> CustomDebugInformations
        {
            get
            {
                if (custom_infos == null)
                    Interlocked.CompareExchange(ref custom_infos, new(), null);

                return custom_infos;
            }
        }
        internal DebugInformation() { }
    }

    public sealed class VariableDebugInformation : DebugInformation
    {
        private ushort attributes;
        internal VariableIndex index;
        public int Index
        {
            get { return index.Index; }
        }
        public string Name { get; set; }
        public VariableAttributes Attributes
        {
            get { return (VariableAttributes)attributes; }
            set { attributes = (ushort)value; }
        }
        public bool IsDebuggerHidden
        {
            get { return attributes.GetAttributes((ushort)VariableAttributes.DebuggerHidden); }
            set { attributes = attributes.SetAttributes((ushort)VariableAttributes.DebuggerHidden, value); }
        }

        internal VariableDebugInformation(int index, string name)
        {
            if (name == null)
                throw new ArgumentNullException("name");

            this.index = new(index);
            this.Name = name;
        }

        public VariableDebugInformation(VariableDefinition variable, string name)
        {
            if (variable == null)
                throw new ArgumentNullException("variable");
            if (name == null)
                throw new ArgumentNullException("name");

            index = new(variable);
            this.Name = name;
            token = new(TokenType.LocalVariable);
        }
    }

    public sealed class ConstantDebugInformation : DebugInformation
    {
        public string Name { get; set; }
        public TypeReference ConstantType { get; set; }
        public object Value { get; set; }

        public ConstantDebugInformation(string name, TypeReference constant_type, object value)
        {
            if (name == null)
                throw new ArgumentNullException("name");

            this.Name = name;
            this.ConstantType = constant_type;
            this.Value = value;
            token = new(TokenType.LocalConstant);
        }
    }

    public enum ImportTargetKind : byte
    {
        ImportNamespace = 1,
        ImportNamespaceInAssembly = 2,
        ImportType = 3,
        ImportXmlNamespaceWithAlias = 4,
        ImportAlias = 5,
        DefineAssemblyAlias = 6,
        DefineNamespaceAlias = 7,
        DefineNamespaceInAssemblyAlias = 8,
        DefineTypeAlias = 9
    }

    public sealed class ImportTarget
    {
        internal ImportTargetKind kind;
        internal string @namespace;
        internal TypeReference type;
        internal AssemblyNameReference reference;
        internal string alias;
        public string Namespace
        {
            get { return @namespace; }
            set { @namespace = value; }
        }
        public TypeReference Type
        {
            get { return type; }
            set { type = value; }
        }
        public AssemblyNameReference AssemblyReference
        {
            get { return reference; }
            set { reference = value; }
        }
        public string Alias
        {
            get { return alias; }
            set { alias = value; }
        }
        public ImportTargetKind Kind
        {
            get { return kind; }
            set { kind = value; }
        }

        public ImportTarget(ImportTargetKind kind)
        {
            this.kind = kind;
        }
    }

    public sealed class ImportDebugInformation : DebugInformation
    {
        internal ImportDebugInformation parent;
        internal Collection<ImportTarget> targets;
        public bool HasTargets
        {
            get { return !targets.IsNullOrEmpty(); }
        }
        public Collection<ImportTarget> Targets
        {
            get
            {
                if (targets == null)
                    Interlocked.CompareExchange(ref targets, new(), null);

                return targets;
            }
        }
        public ImportDebugInformation Parent
        {
            get { return parent; }
            set { parent = value; }
        }

        public ImportDebugInformation()
        {
            token = new(TokenType.ImportScope);
        }
    }

    public interface ICustomDebugInformationProvider : IMetadataTokenProvider
    {
        bool HasCustomDebugInformations { get; }
        Collection<CustomDebugInformation> CustomDebugInformations { get; }
    }

    public enum CustomDebugInformationKind
    {
        Binary,
        StateMachineScope,
        DynamicVariable,
        DefaultNamespace,
        AsyncMethodBody,
        EmbeddedSource,
        SourceLink
    }

    public abstract class CustomDebugInformation : DebugInformation
    {
        public Guid Identifier { get; }
        public abstract CustomDebugInformationKind Kind { get; }

        internal CustomDebugInformation(Guid identifier)
        {
            this.Identifier = identifier;
            token = new(TokenType.CustomDebugInformation);
        }
    }

    public sealed class BinaryCustomDebugInformation : CustomDebugInformation
    {
        public byte[] Data { get; set; }
        public override CustomDebugInformationKind Kind
        {
            get { return CustomDebugInformationKind.Binary; }
        }

        public BinaryCustomDebugInformation(Guid identifier, byte[] data) : base(identifier)
        {
            this.Data = data;
        }
    }

    public sealed class AsyncMethodBodyDebugInformation : CustomDebugInformation
    {
        internal InstructionOffset catch_handler;
        internal Collection<InstructionOffset> yields;
        internal Collection<InstructionOffset> resumes;
        internal Collection<MethodDefinition> resume_methods;
        public InstructionOffset CatchHandler
        {
            get { return catch_handler; }
            set { catch_handler = value; }
        }
        public Collection<InstructionOffset> Yields
        {
            get
            {
                if (yields == null)
                    Interlocked.CompareExchange(ref yields, new(), null);

                return yields;
            }
        }
        public Collection<InstructionOffset> Resumes
        {
            get
            {
                if (resumes == null)
                    Interlocked.CompareExchange(ref resumes, new(), null);

                return resumes;
            }
        }
        public Collection<MethodDefinition> ResumeMethods
        {
            get { return resume_methods ?? (resume_methods = new()); }
        }
        public override CustomDebugInformationKind Kind
        {
            get { return CustomDebugInformationKind.AsyncMethodBody; }
        }
        public static Guid KindIdentifier = new("{54FD2AC5-E925-401A-9C2A-F94F171072F8}");

        internal AsyncMethodBodyDebugInformation(int catchHandler) : base(KindIdentifier)
        {
            catch_handler = new(catchHandler);
        }

        public AsyncMethodBodyDebugInformation(Instruction catchHandler) : base(KindIdentifier)
        {
            catch_handler = new(catchHandler);
        }

        public AsyncMethodBodyDebugInformation() : base(KindIdentifier)
        {
            catch_handler = new(-1);
        }
    }

    public sealed class StateMachineScope
    {
        internal InstructionOffset start;
        internal InstructionOffset end;
        public InstructionOffset Start
        {
            get { return start; }
            set { start = value; }
        }
        public InstructionOffset End
        {
            get { return end; }
            set { end = value; }
        }

        internal StateMachineScope(int start, int end)
        {
            this.start = new(start);
            this.end = new(end);
        }

        public StateMachineScope(Instruction start, Instruction end)
        {
            this.start = new(start);
            this.end = end != null ? new(end) : new InstructionOffset();
        }
    }

    public sealed class StateMachineScopeDebugInformation : CustomDebugInformation
    {
        internal Collection<StateMachineScope> scopes;
        public Collection<StateMachineScope> Scopes
        {
            get { return scopes ?? (scopes = new()); }
        }
        public override CustomDebugInformationKind Kind
        {
            get { return CustomDebugInformationKind.StateMachineScope; }
        }
        public static Guid KindIdentifier = new("{6DA9A61E-F8C7-4874-BE62-68BC5630DF71}");
        public StateMachineScopeDebugInformation() : base(KindIdentifier) { }
    }

    public sealed class EmbeddedSourceDebugInformation : CustomDebugInformation
    {
        internal uint index;
        internal MetadataReader debug_reader;
        internal bool resolved;
        internal byte[] content;
        internal bool compress;
        public byte[] Content
        {
            get
            {
                if (!resolved)
                    Resolve();

                return content;
            }
            set
            {
                content = value;
                resolved = true;
            }
        }
        public bool Compress
        {
            get
            {
                if (!resolved)
                    Resolve();

                return compress;
            }
            set
            {
                compress = value;
                resolved = true;
            }
        }
        public override CustomDebugInformationKind Kind
        {
            get { return CustomDebugInformationKind.EmbeddedSource; }
        }
        public static Guid KindIdentifier = new("{0E8A571B-6926-466E-B4AD-8AB04611F5FE}");

        internal EmbeddedSourceDebugInformation(uint index, MetadataReader debug_reader) : base(KindIdentifier)
        {
            this.index = index;
            this.debug_reader = debug_reader;
        }

        public EmbeddedSourceDebugInformation(byte[] content, bool compress) : base(KindIdentifier)
        {
            resolved = true;
            this.content = content;
            this.compress = compress;
        }

        internal byte[] ReadRawEmbeddedSourceDebugInformation()
        {
            if (debug_reader == null)
                throw new InvalidOperationException();

            return debug_reader.ReadRawEmbeddedSourceDebugInformation(index);
        }

        private void Resolve()
        {
            if (resolved)
                return;

            if (debug_reader == null)
                throw new InvalidOperationException();

            Row<byte[], bool> row = debug_reader.ReadEmbeddedSourceDebugInformation(index);
            content = row.Col1;
            compress = row.Col2;
            resolved = true;
        }
    }

    public sealed class SourceLinkDebugInformation : CustomDebugInformation
    {
        internal string content;
        public string Content
        {
            get { return content; }
            set { content = value; }
        }
        public override CustomDebugInformationKind Kind
        {
            get { return CustomDebugInformationKind.SourceLink; }
        }
        public static Guid KindIdentifier = new("{CC110556-A091-4D38-9FEC-25AB9A351A6A}");

        public SourceLinkDebugInformation(string content) : base(KindIdentifier)
        {
            this.content = content;
        }
    }

    public sealed class MethodDebugInformation : DebugInformation
    {
        internal MethodDefinition method;
        internal Collection<SequencePoint> sequence_points;
        internal ScopeDebugInformation scope;
        internal MethodDefinition kickoff_method;
        internal int code_size;
        internal MetadataToken local_var_token;
        public MethodDefinition Method
        {
            get { return method; }
        }
        public bool HasSequencePoints
        {
            get { return !sequence_points.IsNullOrEmpty(); }
        }
        public Collection<SequencePoint> SequencePoints
        {
            get
            {
                if (sequence_points == null)
                    Interlocked.CompareExchange(ref sequence_points, new(), null);

                return sequence_points;
            }
        }
        public ScopeDebugInformation Scope
        {
            get { return scope; }
            set { scope = value; }
        }
        public MethodDefinition StateMachineKickOffMethod
        {
            get { return kickoff_method; }
            set { kickoff_method = value; }
        }

        internal MethodDebugInformation(MethodDefinition method)
        {
            if (method == null)
                throw new ArgumentNullException("method");

            this.method = method;
            token = new(TokenType.MethodDebugInformation, method.MetadataToken.RID);
        }

        public SequencePoint GetSequencePoint(Instruction instruction)
        {
            if (!HasSequencePoints)
                return null;

            for (int i = 0; i < sequence_points.Count; i++)
                if (sequence_points[i].Offset == instruction.Offset)
                    return sequence_points[i];

            return null;
        }

        public IDictionary<Instruction, SequencePoint> GetSequencePointMapping()
        {
            Dictionary<Instruction, SequencePoint> instruction_mapping = new();
            if (!HasSequencePoints || !method.HasBody)
                return instruction_mapping;

            Dictionary<int, SequencePoint> offset_mapping = new(sequence_points.Count);

            for (int i = 0; i < sequence_points.Count; i++)
            {
                if (!offset_mapping.ContainsKey(sequence_points[i].Offset))
                    offset_mapping.Add(sequence_points[i].Offset, sequence_points[i]);
            }

            Collection<Instruction> instructions = method.Body.Instructions;

            for (int i = 0; i < instructions.Count; i++)
            {
                SequencePoint sequence_point;
                if (offset_mapping.TryGetValue(instructions[i].Offset, out sequence_point))
                    instruction_mapping.Add(instructions[i], sequence_point);
            }

            return instruction_mapping;
        }

        public IEnumerable<ScopeDebugInformation> GetScopes()
        {
            if (scope == null)
                return Empty<ScopeDebugInformation>.Array;

            return GetScopes(new[] { scope });
        }

        private static IEnumerable<ScopeDebugInformation> GetScopes(IList<ScopeDebugInformation> scopes)
        {
            for (int i = 0; i < scopes.Count; i++)
            {
                ScopeDebugInformation scope = scopes[i];

                yield return scope;

                if (!scope.HasScopes)
                    continue;

                foreach (ScopeDebugInformation sub_scope in GetScopes(scope.Scopes))
                    yield return sub_scope;
            }
        }

        public bool TryGetName(VariableDefinition variable, out string name)
        {
            name = null;

            bool has_name = false;
            string unique_name = "";

            foreach (ScopeDebugInformation scope in GetScopes())
            {
                string slot_name;
                if (!scope.TryGetName(variable, out slot_name))
                    continue;

                if (!has_name)
                {
                    has_name = true;
                    unique_name = slot_name;
                    continue;
                }

                if (unique_name != slot_name)
                    return false;
            }

            name = unique_name;
            return has_name;
        }
    }

    public interface ISymbolReader : IDisposable
    {
        ISymbolWriterProvider GetWriterProvider();
        bool ProcessDebugHeader(ImageDebugHeader header);
        MethodDebugInformation Read(MethodDefinition method);
    }

    public interface ISymbolReaderProvider
    {
        ISymbolReader GetSymbolReader(ModuleDefinition module, string fileName);
        ISymbolReader GetSymbolReader(ModuleDefinition module, Stream symbolStream);
    }

#if !NET_CORE
    [Serializable]
#endif
    public sealed class SymbolsNotFoundException : FileNotFoundException
    {
        public SymbolsNotFoundException(string message) : base(message) { }
#if !NET_CORE
        private SymbolsNotFoundException(System.Runtime.Serialization.SerializationInfo info, System.Runtime.Serialization.StreamingContext context) : base(info, context) { }
#endif
    }

#if !NET_CORE
    [Serializable]
#endif
    public sealed class SymbolsNotMatchingException : InvalidOperationException
    {
        public SymbolsNotMatchingException(string message) : base(message) { }
#if !NET_CORE
        private SymbolsNotMatchingException(System.Runtime.Serialization.SerializationInfo info, System.Runtime.Serialization.StreamingContext context) : base(info, context) { }
#endif
    }

    public class DefaultSymbolReaderProvider : ISymbolReaderProvider
    {
        private readonly bool throw_if_no_symbol;
        public DefaultSymbolReaderProvider() : this(throwIfNoSymbol: true) { }

        public DefaultSymbolReaderProvider(bool throwIfNoSymbol)
        {
            throw_if_no_symbol = throwIfNoSymbol;
        }

        public ISymbolReader GetSymbolReader(ModuleDefinition module, string fileName)
        {
            if (module.Image.HasDebugTables())
                return null;

            if (module.HasDebugHeader)
            {
                ImageDebugHeader header = module.GetDebugHeader();
                ImageDebugHeaderEntry entry = header.GetEmbeddedPortablePdbEntry();
                if (entry != null)
                    return new EmbeddedPortablePdbReaderProvider().GetSymbolReader(module, fileName);
            }

            string pdb_file_name = Mixin.GetPdbFileName(fileName);

            if (File.Exists(pdb_file_name))
            {
                if (Mixin.IsPortablePdb(Mixin.GetPdbFileName(fileName)))
                    return new PortablePdbReaderProvider().GetSymbolReader(module, fileName);

                try
                {
                    return SymbolProvider.GetReaderProvider(SymbolKind.NativePdb).GetSymbolReader(module, fileName);
                }
                catch (Exception)
                {
                    // We might not include support for native pdbs.
                }
            }

            string mdb_file_name = Mixin.GetMdbFileName(fileName);
            if (File.Exists(mdb_file_name))
            {
                try
                {
                    return SymbolProvider.GetReaderProvider(SymbolKind.Mdb).GetSymbolReader(module, fileName);
                }
                catch (Exception)
                {
                    // We might not include support for mdbs.
                }
            }

            if (throw_if_no_symbol)
                throw new SymbolsNotFoundException(string.Format("No symbol found for file: {0}", fileName));

            return null;
        }

        public ISymbolReader GetSymbolReader(ModuleDefinition module, Stream symbolStream)
        {
            if (module.Image.HasDebugTables())
                return null;

            if (module.HasDebugHeader)
            {
                ImageDebugHeader header = module.GetDebugHeader();
                ImageDebugHeaderEntry entry = header.GetEmbeddedPortablePdbEntry();
                if (entry != null)
                    return new EmbeddedPortablePdbReaderProvider().GetSymbolReader(module, "");
            }

            Mixin.CheckStream(symbolStream);
            Mixin.CheckReadSeek(symbolStream);

            long position = symbolStream.Position;

            const int portablePdbHeader = 0x424a5342;

            BinaryStreamReader reader = new(symbolStream);
            int intHeader = reader.ReadInt32();
            symbolStream.Position = position;

            if (intHeader == portablePdbHeader)
            {
                return new PortablePdbReaderProvider().GetSymbolReader(module, symbolStream);
            }

            const string nativePdbHeader = "Microsoft C/C++ MSF 7.00";

            byte[] bytesHeader = reader.ReadBytes(nativePdbHeader.Length);
            symbolStream.Position = position;
            bool isNativePdb = true;

            for (int i = 0; i < bytesHeader.Length; i++)
            {
                if (bytesHeader[i] != (byte)nativePdbHeader[i])
                {
                    isNativePdb = false;
                    break;
                }
            }

            if (isNativePdb)
            {
                try
                {
                    return SymbolProvider.GetReaderProvider(SymbolKind.NativePdb).GetSymbolReader(module, symbolStream);
                }
                catch (Exception)
                {
                    // We might not include support for native pdbs.
                }
            }

            const long mdbHeader = 0x45e82623fd7fa614;

            long longHeader = reader.ReadInt64();
            symbolStream.Position = position;

            if (longHeader == mdbHeader)
            {
                try
                {
                    return SymbolProvider.GetReaderProvider(SymbolKind.Mdb).GetSymbolReader(module, symbolStream);
                }
                catch (Exception)
                {
                    // We might not include support for mdbs.
                }
            }

            if (throw_if_no_symbol)
                throw new SymbolsNotFoundException(string.Format("No symbols found in stream"));

            return null;
        }
    }

    internal enum SymbolKind
    {
        NativePdb,
        PortablePdb,
        EmbeddedPortablePdb,
        Mdb
    }

    internal static class SymbolProvider
    {
        private static SR.AssemblyName GetSymbolAssemblyName(SymbolKind kind)
        {
            if (kind == SymbolKind.PortablePdb)
                throw new ArgumentException();

            string suffix = GetSymbolNamespace(kind);

            SR.AssemblyName cecil_name = typeof(SymbolProvider).Assembly.GetName();

            SR.AssemblyName name = new()
            {
                Name = cecil_name.Name + "." + suffix,
                Version = cecil_name.Version,
#if NET_CORE
	            CultureName = cecil_name.CultureName,
#else
                CultureInfo = cecil_name.CultureInfo,
#endif
            };

            name.SetPublicKeyToken(cecil_name.GetPublicKeyToken());

            return name;
        }

        private static Type GetSymbolType(SymbolKind kind, string fullname)
        {
            Type type = Type.GetType(fullname);
            if (type != null)
                return type;

            SR.AssemblyName assembly_name = GetSymbolAssemblyName(kind);

            type = Type.GetType(fullname + ", " + assembly_name.FullName);
            if (type != null)
                return type;

            try
            {
                SR.Assembly assembly = SR.Assembly.Load(assembly_name);
                if (assembly != null)
                    return assembly.GetType(fullname);
            }
            catch (FileNotFoundException) { }
            catch (FileLoadException) { }

            return null;
        }

        public static ISymbolReaderProvider GetReaderProvider(SymbolKind kind)
        {
            if (kind == SymbolKind.PortablePdb)
                return new PortablePdbReaderProvider();
            if (kind == SymbolKind.EmbeddedPortablePdb)
                return new EmbeddedPortablePdbReaderProvider();

            string provider_name = GetSymbolTypeName(kind, "ReaderProvider");
            Type type = GetSymbolType(kind, provider_name);
            if (type == null)
                throw new TypeLoadException("Could not find symbol provider type " + provider_name);

            return (ISymbolReaderProvider)Activator.CreateInstance(type);
        }

        private static string GetSymbolTypeName(SymbolKind kind, string name)
        {
            return "MonoFN.Cecil" + "." + GetSymbolNamespace(kind) + "." + kind + name;
        }

        private static string GetSymbolNamespace(SymbolKind kind)
        {
            if (kind == SymbolKind.PortablePdb || kind == SymbolKind.EmbeddedPortablePdb)
                return "Cil";
            if (kind == SymbolKind.NativePdb)
                return "Pdb";
            if (kind == SymbolKind.Mdb)
                return "Mdb";

            throw new ArgumentException();
        }
    }

    public interface ISymbolWriter : IDisposable
    {
        ISymbolReaderProvider GetReaderProvider();
        ImageDebugHeader GetDebugHeader();
        void Write(MethodDebugInformation info);
    }

    public interface ISymbolWriterProvider
    {
        ISymbolWriter GetSymbolWriter(ModuleDefinition module, string fileName);
        ISymbolWriter GetSymbolWriter(ModuleDefinition module, Stream symbolStream);
    }

    public class DefaultSymbolWriterProvider : ISymbolWriterProvider
    {
        public ISymbolWriter GetSymbolWriter(ModuleDefinition module, string fileName)
        {
            ISymbolReader reader = module.SymbolReader;
            if (reader == null)
                throw new InvalidOperationException();

            if (module.Image != null && module.Image.HasDebugTables())
                return null;

            return reader.GetWriterProvider().GetSymbolWriter(module, fileName);
        }

        public ISymbolWriter GetSymbolWriter(ModuleDefinition module, Stream symbolStream)
        {
            throw new NotSupportedException();
        }
    }
}

namespace MonoFN.Cecil
{
    internal static partial class Mixin
    {
        public static ImageDebugHeaderEntry GetCodeViewEntry(this ImageDebugHeader header)
        {
            return GetEntry(header, ImageDebugType.CodeView);
        }

        public static ImageDebugHeaderEntry GetDeterministicEntry(this ImageDebugHeader header)
        {
            return GetEntry(header, ImageDebugType.Deterministic);
        }

        public static ImageDebugHeader AddDeterministicEntry(this ImageDebugHeader header)
        {
            ImageDebugHeaderEntry entry = new(new() { Type = ImageDebugType.Deterministic }, Empty<byte>.Array);
            if (header == null)
                return new(entry);

            ImageDebugHeaderEntry[] entries = new ImageDebugHeaderEntry [header.Entries.Length + 1];
            Array.Copy(header.Entries, entries, header.Entries.Length);
            entries[entries.Length - 1] = entry;
            return new(entries);
        }

        public static ImageDebugHeaderEntry GetEmbeddedPortablePdbEntry(this ImageDebugHeader header)
        {
            return GetEntry(header, ImageDebugType.EmbeddedPortablePdb);
        }

        private static ImageDebugHeaderEntry GetEntry(this ImageDebugHeader header, ImageDebugType type)
        {
            if (!header.HasEntries)
                return null;

            for (int i = 0; i < header.Entries.Length; i++)
            {
                ImageDebugHeaderEntry entry = header.Entries[i];
                if (entry.Directory.Type == type)
                    return entry;
            }

            return null;
        }

        public static string GetPdbFileName(string assemblyFileName)
        {
            return Path.ChangeExtension(assemblyFileName, ".pdb");
        }

        public static string GetMdbFileName(string assemblyFileName)
        {
            return assemblyFileName + ".mdb";
        }

        public static bool IsPortablePdb(string fileName)
        {
            using (FileStream file = new(fileName, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
            {
                return IsPortablePdb(file);
            }
        }

        public static bool IsPortablePdb(Stream stream)
        {
            const uint ppdb_signature = 0x424a5342;

            if (stream.Length < 4)
                return false;
            long position = stream.Position;
            try
            {
                BinaryReader reader = new(stream);
                return reader.ReadUInt32() == ppdb_signature;
            }
            finally
            {
                stream.Position = position;
            }
        }
    }
}