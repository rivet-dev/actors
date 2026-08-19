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
using MonoFN.Cecil.Metadata;
using MonoFN.Cecil.PE;
using MonoFN.Collections.Generic;
using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using SR = System.Reflection;

namespace MonoFN.Cecil
{
    public enum ReadingMode
    {
        Immediate = 1,
        Deferred = 2
    }

    public sealed class ReaderParameters
    {
        internal IAssemblyResolver assembly_resolver;
        internal IMetadataResolver metadata_resolver;
        internal IMetadataImporterProvider metadata_importer_provider;
        internal IReflectionImporterProvider reflection_importer_provider;
        public ReadingMode ReadingMode { get; set; }
        public bool InMemory { get; set; }
        public IAssemblyResolver AssemblyResolver
        {
            get { return assembly_resolver; }
            set { assembly_resolver = value; }
        }
        public IMetadataResolver MetadataResolver
        {
            get { return metadata_resolver; }
            set { metadata_resolver = value; }
        }
        public IMetadataImporterProvider MetadataImporterProvider
        {
            get { return metadata_importer_provider; }
            set { metadata_importer_provider = value; }
        }
        public IReflectionImporterProvider ReflectionImporterProvider
        {
            get { return reflection_importer_provider; }
            set { reflection_importer_provider = value; }
        }
        public Stream SymbolStream { get; set; }
        public ISymbolReaderProvider SymbolReaderProvider { get; set; }
        public bool ReadSymbols { get; set; }
        public bool ThrowIfSymbolsAreNotMatching { get; set; }
        public bool ReadWrite { get; set; }
        public bool ApplyWindowsRuntimeProjections { get; set; }
        public ReaderParameters() : this(ReadingMode.Deferred) { }

        public ReaderParameters(ReadingMode readingMode)
        {
            ReadingMode = readingMode;
            ThrowIfSymbolsAreNotMatching = true;
        }
    }

    public sealed class ModuleParameters
    {
        public ModuleKind Kind { get; set; }
        public TargetRuntime Runtime { get; set; }
        public uint? Timestamp { get; set; }
        public TargetArchitecture Architecture { get; set; }
        public IAssemblyResolver AssemblyResolver { get; set; }
        public IMetadataResolver MetadataResolver { get; set; }
        public IMetadataImporterProvider MetadataImporterProvider { get; set; }
        public IReflectionImporterProvider ReflectionImporterProvider { get; set; }

        public ModuleParameters()
        {
            Kind = ModuleKind.Dll;
            Runtime = GetCurrentRuntime();
            Architecture = TargetArchitecture.I386;
        }

        private static TargetRuntime GetCurrentRuntime()
        {
            return typeof(object).Assembly.ImageRuntimeVersion.ParseRuntime();
        }
    }

    public sealed class WriterParameters
    {
        public uint? Timestamp { get; set; }
        public Stream SymbolStream { get; set; }
        public ISymbolWriterProvider SymbolWriterProvider { get; set; }
        public bool WriteSymbols { get; set; }
        public bool HasStrongNameKey
        {
            get { return StrongNameKeyPair != null || StrongNameKeyBlob != null || StrongNameKeyContainer != null; }
        }
        public byte[] StrongNameKeyBlob { get; set; }
        public string StrongNameKeyContainer { get; set; }
        public SR.StrongNameKeyPair StrongNameKeyPair { get; set; }
        public bool DeterministicMvid { get; set; }
    }

    public sealed class ModuleDefinition : ModuleReference, ICustomAttributeProvider, ICustomDebugInformationProvider, IDisposable
    {
        internal Image Image;
        internal MetadataSystem MetadataSystem;
        internal ReadingMode ReadingMode;
        internal ISymbolReaderProvider SymbolReaderProvider;
        internal ISymbolReader symbol_reader;
        internal Disposable<IAssemblyResolver> assembly_resolver;
        internal IMetadataResolver metadata_resolver;
        internal TypeSystem type_system;
        internal readonly MetadataReader reader;
        internal string runtime_version;
        internal ModuleKind kind;
        private WindowsRuntimeProjections projections;
        private TargetRuntime runtime;
        internal ushort linker_version = 8;
        internal ushort subsystem_major = 4;
        internal ushort subsystem_minor = 0;
        internal uint timestamp;
        internal AssemblyDefinition assembly;
        private MethodDefinition entry_point;
        private bool entry_point_set;
        internal IReflectionImporter reflection_importer;
        internal IMetadataImporter metadata_importer;
        private Collection<CustomAttribute> custom_attributes;
        private Collection<AssemblyNameReference> references;
        private Collection<ModuleReference> modules;
        private Collection<Resource> resources;
        private Collection<ExportedType> exported_types;
        private TypeDefinitionCollection types;
        internal Collection<CustomDebugInformation> custom_infos;
        internal MetadataBuilder metadata_builder;
        public bool IsMain
        {
            get { return kind != ModuleKind.NetModule; }
        }
        public ModuleKind Kind
        {
            get { return kind; }
            set { kind = value; }
        }
        public MetadataKind MetadataKind { get; set; }
        internal WindowsRuntimeProjections Projections
        {
            get
            {
                if (projections == null)
                    Interlocked.CompareExchange(ref projections, new(this), null);

                return projections;
            }
        }
        public TargetRuntime Runtime
        {
            get { return runtime; }
            set
            {
                runtime = value;
                runtime_version = runtime.RuntimeVersionString();
            }
        }
        public string RuntimeVersion
        {
            get { return runtime_version; }
            set
            {
                runtime_version = value;
                runtime = runtime_version.ParseRuntime();
            }
        }
        public TargetArchitecture Architecture { get; set; }
        public ModuleAttributes Attributes { get; set; }
        public ModuleCharacteristics Characteristics { get; set; }
        [Obsolete("Use FileName")]
        public string FullyQualifiedName
        {
            get { return FileName; }
        }
        public string FileName { get; }
        public Guid Mvid { get; set; }
        internal bool HasImage
        {
            get { return Image != null; }
        }
        public bool HasSymbols
        {
            get { return symbol_reader != null; }
        }
        public ISymbolReader SymbolReader
        {
            get { return symbol_reader; }
        }
        public override MetadataScopeType MetadataScopeType
        {
            get { return MetadataScopeType.ModuleDefinition; }
        }
        public AssemblyDefinition Assembly
        {
            get { return assembly; }
        }
        internal IReflectionImporter ReflectionImporter
        {
            get
            {
                if (reflection_importer == null)
                    Interlocked.CompareExchange(ref reflection_importer, new DefaultReflectionImporter(this), null);

                return reflection_importer;
            }
        }
        internal IMetadataImporter MetadataImporter
        {
            get
            {
                if (metadata_importer == null)
                    Interlocked.CompareExchange(ref metadata_importer, new DefaultMetadataImporter(this), null);

                return metadata_importer;
            }
        }
        public IAssemblyResolver AssemblyResolver
        {
            get
            {
                if (assembly_resolver.value == null)
                {
                    lock (SyncRoot)
                    {
                        assembly_resolver = Disposable.Owned(new DefaultAssemblyResolver() as IAssemblyResolver);
                    }
                }

                return assembly_resolver.value;
            }
        }
        public IMetadataResolver MetadataResolver
        {
            get
            {
                if (metadata_resolver == null)
                    Interlocked.CompareExchange(ref metadata_resolver, new MetadataResolver(AssemblyResolver), null);

                return metadata_resolver;
            }
        }
        public TypeSystem TypeSystem
        {
            get
            {
                if (type_system == null)
                    Interlocked.CompareExchange(ref type_system, TypeSystem.CreateTypeSystem(this), null);

                return type_system;
            }
        }
        public bool HasAssemblyReferences
        {
            get
            {
                if (references != null)
                    return references.Count > 0;

                return HasImage && Image.HasTable(Table.AssemblyRef);
            }
        }
        public Collection<AssemblyNameReference> AssemblyReferences
        {
            get
            {
                if (references != null)
                    return references;

                if (HasImage)
                    return Read(ref references, this, (_, reader) => reader.ReadAssemblyReferences());

                Interlocked.CompareExchange(ref references, new(), null);
                return references;
            }
        }
        public bool HasModuleReferences
        {
            get
            {
                if (modules != null)
                    return modules.Count > 0;

                return HasImage && Image.HasTable(Table.ModuleRef);
            }
        }
        public Collection<ModuleReference> ModuleReferences
        {
            get
            {
                if (modules != null)
                    return modules;

                if (HasImage)
                    return Read(ref modules, this, (_, reader) => reader.ReadModuleReferences());

                Interlocked.CompareExchange(ref modules, new(), null);
                return modules;
            }
        }
        public bool HasResources
        {
            get
            {
                if (resources != null)
                    return resources.Count > 0;

                if (HasImage)
                    return Image.HasTable(Table.ManifestResource) || Read(this, (_, reader) => reader.HasFileResource());

                return false;
            }
        }
        public Collection<Resource> Resources
        {
            get
            {
                if (resources != null)
                    return resources;

                if (HasImage)
                    return Read(ref resources, this, (_, reader) => reader.ReadResources());

                Interlocked.CompareExchange(ref resources, new(), null);
                return resources;
            }
        }
        public bool HasCustomAttributes
        {
            get
            {
                if (custom_attributes != null)
                    return custom_attributes.Count > 0;

                return this.GetHasCustomAttributes(this);
            }
        }
        public Collection<CustomAttribute> CustomAttributes
        {
            get { return custom_attributes ?? this.GetCustomAttributes(ref custom_attributes, this); }
        }
        public bool HasTypes
        {
            get
            {
                if (types != null)
                    return types.Count > 0;

                return HasImage && Image.HasTable(Table.TypeDef);
            }
        }
        public Collection<TypeDefinition> Types
        {
            get
            {
                if (types != null)
                    return types;

                if (HasImage)
                    return Read(ref types, this, (_, reader) => reader.ReadTypes());

                Interlocked.CompareExchange(ref types, new(this), null);
                return types;
            }
        }
        public bool HasExportedTypes
        {
            get
            {
                if (exported_types != null)
                    return exported_types.Count > 0;

                return HasImage && Image.HasTable(Table.ExportedType);
            }
        }
        public Collection<ExportedType> ExportedTypes
        {
            get
            {
                if (exported_types != null)
                    return exported_types;

                if (HasImage)
                    return Read(ref exported_types, this, (_, reader) => reader.ReadExportedTypes());

                Interlocked.CompareExchange(ref exported_types, new(), null);
                return exported_types;
            }
        }
        public MethodDefinition EntryPoint
        {
            get
            {
                if (entry_point_set)
                    return entry_point;

                if (HasImage)
                    Read(ref entry_point, this, (_, reader) => reader.ReadEntryPoint());
                else
                    entry_point = null;

                entry_point_set = true;
                return entry_point;
            }
            set
            {
                entry_point = value;
                entry_point_set = true;
            }
        }
        public bool HasCustomDebugInformations
        {
            get { return custom_infos != null && custom_infos.Count > 0; }
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

        internal ModuleDefinition()
        {
            MetadataSystem = new();
            token = new(TokenType.Module, 1);
        }

        internal ModuleDefinition(Image image) : this()
        {
            Image = image;
            kind = image.Kind;
            RuntimeVersion = image.RuntimeVersion;
            Architecture = image.Architecture;
            Attributes = image.Attributes;
            Characteristics = image.DllCharacteristics;
            linker_version = image.LinkerVersion;
            subsystem_major = image.SubSystemMajor;
            subsystem_minor = image.SubSystemMinor;
            FileName = image.FileName;
            timestamp = image.Timestamp;

            reader = new(this);
        }

        public void Dispose()
        {
            if (Image != null)
                Image.Dispose();

            if (symbol_reader != null)
                symbol_reader.Dispose();

            if (assembly_resolver.value != null)
                assembly_resolver.Dispose();
        }

        public bool HasTypeReference(string fullName)
        {
            return HasTypeReference(string.Empty, fullName);
        }

        public bool HasTypeReference(string scope, string fullName)
        {
            Mixin.CheckFullName(fullName);

            if (!HasImage)
                return false;

            return GetTypeReference(scope, fullName) != null;
        }

        public bool TryGetTypeReference(string fullName, out TypeReference type)
        {
            return TryGetTypeReference(string.Empty, fullName, out type);
        }

        public bool TryGetTypeReference(string scope, string fullName, out TypeReference type)
        {
            Mixin.CheckFullName(fullName);

            if (!HasImage)
            {
                type = null;
                return false;
            }

            return (type = GetTypeReference(scope, fullName)) != null;
        }

        private TypeReference GetTypeReference(string scope, string fullname)
        {
            return Read(new Row<string, string>(scope, fullname), (row, reader) => reader.GetTypeReference(row.Col1, row.Col2));
        }

        public IEnumerable<TypeReference> GetTypeReferences()
        {
            if (!HasImage)
                return Empty<TypeReference>.Array;

            return Read(this, (_, reader) => reader.GetTypeReferences());
        }

        public IEnumerable<MemberReference> GetMemberReferences()
        {
            if (!HasImage)
                return Empty<MemberReference>.Array;

            return Read(this, (_, reader) => reader.GetMemberReferences());
        }

        public IEnumerable<CustomAttribute> GetCustomAttributes()
        {
            if (!HasImage)
                return Empty<CustomAttribute>.Array;

            return Read(this, (_, reader) => reader.GetCustomAttributes());
        }

        public TypeReference GetType(string fullName, bool runtimeName)
        {
            return runtimeName ? TypeParser.ParseType(this, fullName, typeDefinitionOnly: true) : GetType(fullName);
        }

        public TypeDefinition GetType(string fullName)
        {
            Mixin.CheckFullName(fullName);

            int position = fullName.IndexOf('/');
            if (position > 0)
                return GetNestedType(fullName);

            return ((TypeDefinitionCollection)Types).GetType(fullName);
        }

        public TypeDefinition GetType(string @namespace, string name)
        {
            Mixin.CheckName(name);

            return ((TypeDefinitionCollection)Types).GetType(@namespace ?? string.Empty, name);
        }

        public IEnumerable<TypeDefinition> GetTypes()
        {
            return GetTypes(Types);
        }

        private static IEnumerable<TypeDefinition> GetTypes(Collection<TypeDefinition> types)
        {
            for (int i = 0; i < types.Count; i++)
            {
                TypeDefinition type = types[i];

                yield return type;

                if (!type.HasNestedTypes)
                    continue;

                foreach (TypeDefinition nested in GetTypes(type.NestedTypes))
                    yield return nested;
            }
        }

        private TypeDefinition GetNestedType(string fullname)
        {
            string[] names = fullname.Split('/');
            TypeDefinition type = GetType(names[0]);

            if (type == null)
                return null;

            for (int i = 1; i < names.Length; i++)
            {
                TypeDefinition nested_type = type.GetNestedType(names[i]);
                if (nested_type == null)
                    return null;

                type = nested_type;
            }

            return type;
        }

        internal FieldDefinition Resolve(FieldReference field)
        {
            return MetadataResolver.Resolve(field);
        }

        internal MethodDefinition Resolve(MethodReference method)
        {
            return MetadataResolver.Resolve(method);
        }

        internal TypeDefinition Resolve(TypeReference type)
        {
            return MetadataResolver.Resolve(type);
        }

        private static void CheckContext(IGenericParameterProvider context, ModuleDefinition module)
        {
            if (context == null)
                return;

            if (context.Module != module)
                throw new ArgumentException();
        }

        [Obsolete("Use ImportReference", error: false)]
        public TypeReference Import(Type type)
        {
            return ImportReference(type, null);
        }

        public TypeReference ImportReference(Type type)
        {
            return ImportReference(type, null);
        }

        [Obsolete("Use ImportReference", error: false)]
        public TypeReference Import(Type type, IGenericParameterProvider context)
        {
            return ImportReference(type, context);
        }

        public TypeReference ImportReference(Type type, IGenericParameterProvider context)
        {
            Mixin.CheckType(type);
            CheckContext(context, this);

            return ReflectionImporter.ImportReference(type, context);
        }

        [Obsolete("Use ImportReference", error: false)]
        public FieldReference Import(SR.FieldInfo field)
        {
            return ImportReference(field, null);
        }

        [Obsolete("Use ImportReference", error: false)]
        public FieldReference Import(SR.FieldInfo field, IGenericParameterProvider context)
        {
            return ImportReference(field, context);
        }

        public FieldReference ImportReference(SR.FieldInfo field)
        {
            return ImportReference(field, null);
        }

        public FieldReference ImportReference(SR.FieldInfo field, IGenericParameterProvider context)
        {
            Mixin.CheckField(field);
            CheckContext(context, this);

            return ReflectionImporter.ImportReference(field, context);
        }

        [Obsolete("Use ImportReference", error: false)]
        public MethodReference Import(SR.MethodBase method)
        {
            return ImportReference(method, null);
        }

        [Obsolete("Use ImportReference", error: false)]
        public MethodReference Import(SR.MethodBase method, IGenericParameterProvider context)
        {
            return ImportReference(method, context);
        }

        public MethodReference ImportReference(SR.MethodBase method)
        {
            return ImportReference(method, null);
        }

        public MethodReference ImportReference(SR.MethodBase method, IGenericParameterProvider context)
        {
            Mixin.CheckMethod(method);
            CheckContext(context, this);

            return ReflectionImporter.ImportReference(method, context);
        }

        [Obsolete("Use ImportReference", error: false)]
        public TypeReference Import(TypeReference type)
        {
            return ImportReference(type, null);
        }

        [Obsolete("Use ImportReference", error: false)]
        public TypeReference Import(TypeReference type, IGenericParameterProvider context)
        {
            return ImportReference(type, context);
        }

        public TypeReference ImportReference(TypeReference type)
        {
            return ImportReference(type, null);
        }

        public TypeReference ImportReference(TypeReference type, IGenericParameterProvider context)
        {
            Mixin.CheckType(type);

            if (type.Module == this)
                return type;

            CheckContext(context, this);

            return MetadataImporter.ImportReference(type, context);
        }

        [Obsolete("Use ImportReference", error: false)]
        public FieldReference Import(FieldReference field)
        {
            return ImportReference(field, null);
        }

        [Obsolete("Use ImportReference", error: false)]
        public FieldReference Import(FieldReference field, IGenericParameterProvider context)
        {
            return ImportReference(field, context);
        }

        public FieldReference ImportReference(FieldReference field)
        {
            return ImportReference(field, null);
        }

        public FieldReference ImportReference(FieldReference field, IGenericParameterProvider context)
        {
            Mixin.CheckField(field);

            if (field.Module == this)
                return field;

            CheckContext(context, this);

            return MetadataImporter.ImportReference(field, context);
        }

        [Obsolete("Use ImportReference", error: false)]
        public MethodReference Import(MethodReference method)
        {
            return ImportReference(method, null);
        }

        [Obsolete("Use ImportReference", error: false)]
        public MethodReference Import(MethodReference method, IGenericParameterProvider context)
        {
            return ImportReference(method, context);
        }

        public MethodReference ImportReference(MethodReference method)
        {
            return ImportReference(method, null);
        }

        public MethodReference ImportReference(MethodReference method, IGenericParameterProvider context)
        {
            Mixin.CheckMethod(method);

            if (method.Module == this)
                return method;

            CheckContext(context, this);

            return MetadataImporter.ImportReference(method, context);
        }

        public IMetadataTokenProvider LookupToken(int token)
        {
            return LookupToken(new MetadataToken((uint)token));
        }

        public IMetadataTokenProvider LookupToken(MetadataToken token)
        {
            return Read(token, (t, reader) => reader.LookupToken(t));
        }

        public void ImmediateRead()
        {
            if (!HasImage)
                return;
            ReadingMode = ReadingMode.Immediate;
            ImmediateModuleReader moduleReader = new(Image);
            moduleReader.ReadModule(this, resolve_attributes: true);
        }

        internal object SyncRoot { get; } = new();

        internal void Read<TItem>(TItem item, Action<TItem, MetadataReader> read)
        {
            lock (SyncRoot)
            {
                int position = reader.position;
                IGenericContext context = reader.context;

                read(item, reader);

                reader.position = position;
                reader.context = context;
            }
        }

        internal TRet Read<TItem, TRet>(TItem item, Func<TItem, MetadataReader, TRet> read)
        {
            lock (SyncRoot)
            {
                int position = reader.position;
                IGenericContext context = reader.context;

                TRet ret = read(item, reader);

                reader.position = position;
                reader.context = context;

                return ret;
            }
        }

        internal TRet Read<TItem, TRet>(ref TRet variable, TItem item, Func<TItem, MetadataReader, TRet> read) where TRet : class
        {
            lock (SyncRoot)
            {
                if (variable != null)
                    return variable;

                int position = reader.position;
                IGenericContext context = reader.context;

                TRet ret = read(item, reader);

                reader.position = position;
                reader.context = context;

                return variable = ret;
            }
        }

        public bool HasDebugHeader
        {
            get { return Image != null && Image.DebugHeader != null; }
        }

        public ImageDebugHeader GetDebugHeader()
        {
            return Image.DebugHeader ?? new ImageDebugHeader();
        }

        public static ModuleDefinition CreateModule(string name, ModuleKind kind)
        {
            return CreateModule(name, new ModuleParameters { Kind = kind });
        }

        public static ModuleDefinition CreateModule(string name, ModuleParameters parameters)
        {
            Mixin.CheckName(name);
            Mixin.CheckParameters(parameters);

            ModuleDefinition module = new()
            {
                Name = name,
                kind = parameters.Kind,
                timestamp = parameters.Timestamp ?? Mixin.GetTimestamp(),
                Runtime = parameters.Runtime,
                Architecture = parameters.Architecture,
                Mvid = Guid.NewGuid(),
                Attributes = ModuleAttributes.ILOnly,
                Characteristics = (ModuleCharacteristics)0x8540
            };

            if (parameters.AssemblyResolver != null)
                module.assembly_resolver = Disposable.NotOwned(parameters.AssemblyResolver);

            if (parameters.MetadataResolver != null)
                module.metadata_resolver = parameters.MetadataResolver;

            if (parameters.MetadataImporterProvider != null)
                module.metadata_importer = parameters.MetadataImporterProvider.GetMetadataImporter(module);

            if (parameters.ReflectionImporterProvider != null)
                module.reflection_importer = parameters.ReflectionImporterProvider.GetReflectionImporter(module);

            if (parameters.Kind != ModuleKind.NetModule)
            {
                AssemblyDefinition assembly = new();
                module.assembly = assembly;
                module.assembly.Name = CreateAssemblyName(name);
                assembly.main_module = module;
            }

            module.Types.Add(new(string.Empty, "<Module>", TypeAttributes.NotPublic));

            return module;
        }

        private static AssemblyNameDefinition CreateAssemblyName(string name)
        {
            if (name.EndsWith(".dll") || name.EndsWith(".exe"))
                name = name.Substring(0, name.Length - 4);

            return new(name, Mixin.ZeroVersion);
        }

        public void ReadSymbols()
        {
            if (string.IsNullOrEmpty(FileName))
                throw new InvalidOperationException();

            DefaultSymbolReaderProvider provider = new(throwIfNoSymbol: true);
            ReadSymbols(provider.GetSymbolReader(this, FileName), throwIfSymbolsAreNotMaching: true);
        }

        public void ReadSymbols(ISymbolReader reader)
        {
            ReadSymbols(reader, throwIfSymbolsAreNotMaching: true);
        }

        public void ReadSymbols(ISymbolReader reader, bool throwIfSymbolsAreNotMaching)
        {
            if (reader == null)
                throw new ArgumentNullException("reader");

            symbol_reader = reader;

            if (!symbol_reader.ProcessDebugHeader(GetDebugHeader()))
            {
                symbol_reader = null;

                if (throwIfSymbolsAreNotMaching)
                    throw new SymbolsNotMatchingException("Symbols were found but are not matching the assembly");

                return;
            }

            if (HasImage && ReadingMode == ReadingMode.Immediate)
            {
                ImmediateModuleReader immediate_reader = new(Image);
                immediate_reader.ReadSymbols(this);
            }
        }

        public static ModuleDefinition ReadModule(string fileName)
        {
            return ReadModule(fileName, new(ReadingMode.Deferred));
        }

        public static ModuleDefinition ReadModule(string fileName, ReaderParameters parameters)
        {
            Stream stream = GetFileStream(fileName, FileMode.Open, parameters.ReadWrite ? FileAccess.ReadWrite : FileAccess.Read, FileShare.Read);

            if (parameters.InMemory)
            {
                MemoryStream memory = new(stream.CanSeek ? (int)stream.Length : 0);
                using (stream)
                {
                    stream.CopyTo(memory);
                }

                memory.Position = 0;
                stream = memory;
            }

            try
            {
                return ReadModule(Disposable.Owned(stream), fileName, parameters);
            }
            catch (Exception)
            {
                stream.Dispose();
                throw;
            }
        }

        private static Stream GetFileStream(string fileName, FileMode mode, FileAccess access, FileShare share)
        {
            Mixin.CheckFileName(fileName);

            return new FileStream(fileName, mode, access, share);
        }

        public static ModuleDefinition ReadModule(Stream stream)
        {
            return ReadModule(stream, new(ReadingMode.Deferred));
        }

        public static ModuleDefinition ReadModule(Stream stream, ReaderParameters parameters)
        {
            Mixin.CheckStream(stream);
            Mixin.CheckReadSeek(stream);

            return ReadModule(Disposable.NotOwned(stream), stream.GetFileName(), parameters);
        }

        private static ModuleDefinition ReadModule(Disposable<Stream> stream, string fileName, ReaderParameters parameters)
        {
            Mixin.CheckParameters(parameters);

            return ModuleReader.CreateModule(ImageReader.ReadImage(stream, fileName), parameters);
        }

        public void Write(string fileName)
        {
            Write(fileName, new());
        }

        public void Write(string fileName, WriterParameters parameters)
        {
            Mixin.CheckParameters(parameters);
            Stream file = GetFileStream(fileName, FileMode.Create, FileAccess.ReadWrite, FileShare.Read);
            ModuleWriter.WriteModule(this, Disposable.Owned(file), parameters);
        }

        public void Write()
        {
            Write(new WriterParameters());
        }

        public void Write(WriterParameters parameters)
        {
            if (!HasImage)
                throw new InvalidOperationException();

            Write(Image.Stream.value, parameters);
        }

        public void Write(Stream stream)
        {
            Write(stream, new());
        }

        public void Write(Stream stream, WriterParameters parameters)
        {
            Mixin.CheckStream(stream);
            Mixin.CheckWriteSeek(stream);
            Mixin.CheckParameters(parameters);

            ModuleWriter.WriteModule(this, Disposable.NotOwned(stream), parameters);
        }
    }

    internal static partial class Mixin
    {
        public enum Argument
        {
            name,
            fileName,
            fullName,
            stream,
            type,
            method,
            field,
            parameters,
            module,
            modifierType,
            eventType,
            fieldType,
            declaringType,
            returnType,
            propertyType,
            interfaceType,
            constraintType
        }

        public static void CheckName(object name)
        {
            if (name == null)
                throw new ArgumentNullException(Argument.name.ToString());
        }

        public static void CheckName(string name)
        {
            if (string.IsNullOrEmpty(name))
                throw new ArgumentNullOrEmptyException(Argument.name.ToString());
        }

        public static void CheckFileName(string fileName)
        {
            if (string.IsNullOrEmpty(fileName))
                throw new ArgumentNullOrEmptyException(Argument.fileName.ToString());
        }

        public static void CheckFullName(string fullName)
        {
            if (string.IsNullOrEmpty(fullName))
                throw new ArgumentNullOrEmptyException(Argument.fullName.ToString());
        }

        public static void CheckStream(object stream)
        {
            if (stream == null)
                throw new ArgumentNullException(Argument.stream.ToString());
        }

        public static void CheckWriteSeek(Stream stream)
        {
            if (!stream.CanWrite || !stream.CanSeek)
                throw new ArgumentException("Stream must be writable and seekable.");
        }

        public static void CheckReadSeek(Stream stream)
        {
            if (!stream.CanRead || !stream.CanSeek)
                throw new ArgumentException("Stream must be readable and seekable.");
        }

        public static void CheckType(object type)
        {
            if (type == null)
                throw new ArgumentNullException(Argument.type.ToString());
        }

        public static void CheckType(object type, Argument argument)
        {
            if (type == null)
                throw new ArgumentNullException(argument.ToString());
        }

        public static void CheckField(object field)
        {
            if (field == null)
                throw new ArgumentNullException(Argument.field.ToString());
        }

        public static void CheckMethod(object method)
        {
            if (method == null)
                throw new ArgumentNullException(Argument.method.ToString());
        }

        public static void CheckParameters(object parameters)
        {
            if (parameters == null)
                throw new ArgumentNullException(Argument.parameters.ToString());
        }

        public static uint GetTimestamp()
        {
            return (uint)DateTime.UtcNow.Subtract(new DateTime(1970, 1, 1)).TotalSeconds;
        }

        public static bool HasImage(this ModuleDefinition self)
        {
            return self != null && self.HasImage;
        }

        public static string GetFileName(this Stream self)
        {
            FileStream file_stream = self as FileStream;
            if (file_stream == null)
                return string.Empty;

            return Path.GetFullPath(file_stream.Name);
        }

        public static TargetRuntime ParseRuntime(this string self)
        {
            if (string.IsNullOrEmpty(self))
                return TargetRuntime.Net_4_0;

            switch (self[1])
            {
                case '1':
                    return self[3] == '0' ? TargetRuntime.Net_1_0 : TargetRuntime.Net_1_1;
                case '2':
                    return TargetRuntime.Net_2_0;
                case '4':
                default:
                    return TargetRuntime.Net_4_0;
            }
        }

        public static string RuntimeVersionString(this TargetRuntime runtime)
        {
            switch (runtime)
            {
                case TargetRuntime.Net_1_0:
                    return "v1.0.3705";
                case TargetRuntime.Net_1_1:
                    return "v1.1.4322";
                case TargetRuntime.Net_2_0:
                    return "v2.0.50727";
                case TargetRuntime.Net_4_0:
                default:
                    return "v4.0.30319";
            }
        }

        public static bool IsWindowsMetadata(this ModuleDefinition module)
        {
            return module.MetadataKind != MetadataKind.Ecma335;
        }

        public static byte[] ReadAll(this Stream self)
        {
            int read;
            MemoryStream memory = new((int)self.Length);
            byte[] buffer = new byte [1024];

            while ((read = self.Read(buffer, 0, buffer.Length)) != 0)
                memory.Write(buffer, 0, read);

            return memory.ToArray();
        }

        public static void Read(object o) { }
    }
}