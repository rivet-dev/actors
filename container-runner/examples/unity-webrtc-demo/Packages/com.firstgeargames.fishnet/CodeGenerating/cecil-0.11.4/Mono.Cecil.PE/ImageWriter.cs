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
using System;
using System.IO;
using RVA = System.UInt32;

namespace MonoFN.Cecil.PE
{
    internal sealed class ImageWriter : BinaryStreamWriter
    {
        private readonly ModuleDefinition module;
        private readonly MetadataBuilder metadata;
        private readonly TextMap text_map;
        internal readonly Disposable<Stream> stream;
        private readonly string runtime_version;
        private ImageDebugHeader debug_header;
        private ByteBuffer win32_resources;
        private const uint pe_header_size = 0x98u;
        private const uint section_header_size = 0x28u;
        private const uint file_alignment = 0x200;
        private const uint section_alignment = 0x2000;
        private const ulong image_base = 0x00400000;
        internal const RVA text_rva = 0x2000;
        private readonly bool pe64;
        private readonly bool has_reloc;
        internal Section text;
        internal Section rsrc;
        internal Section reloc;
        private ushort sections;

        private ImageWriter(ModuleDefinition module, string runtime_version, MetadataBuilder metadata, Disposable<Stream> stream, bool metadataOnly = false) : base(stream.value)
        {
            this.module = module;
            this.runtime_version = runtime_version;
            text_map = metadata.text_map;
            this.stream = stream;
            this.metadata = metadata;
            if (metadataOnly)
                return;

            pe64 = module.Architecture == TargetArchitecture.AMD64 || module.Architecture == TargetArchitecture.IA64 || module.Architecture == TargetArchitecture.ARM64;
            has_reloc = module.Architecture == TargetArchitecture.I386;
            GetDebugHeader();
            GetWin32Resources();
            BuildTextMap();
            sections = (ushort)(has_reloc ? 2 : 1); // text + reloc?
        }

        private void GetDebugHeader()
        {
            ISymbolWriter symbol_writer = metadata.symbol_writer;
            if (symbol_writer != null)
                debug_header = symbol_writer.GetDebugHeader();

            if (module.HasDebugHeader)
            {
                ImageDebugHeader header = module.GetDebugHeader();
                ImageDebugHeaderEntry deterministic = header.GetDeterministicEntry();
                if (deterministic == null)
                    return;

                debug_header = debug_header.AddDeterministicEntry();
            }
        }

        private void GetWin32Resources()
        {
            if (!module.HasImage)
                return;

            DataDirectory win32_resources_directory = module.Image.Win32Resources;
            uint size = win32_resources_directory.Size;

            if (size > 0)
            {
                win32_resources = module.Image.GetReaderAt(win32_resources_directory.VirtualAddress, size, (s, reader) => new ByteBuffer(reader.ReadBytes((int)s)));
            }
        }

        public static ImageWriter CreateWriter(ModuleDefinition module, MetadataBuilder metadata, Disposable<Stream> stream)
        {
            ImageWriter writer = new(module, module.runtime_version, metadata, stream);
            writer.BuildSections();
            return writer;
        }

        public static ImageWriter CreateDebugWriter(ModuleDefinition module, MetadataBuilder metadata, Disposable<Stream> stream)
        {
            ImageWriter writer = new(module, "PDB v1.0", metadata, stream, metadataOnly: true);
            uint length = metadata.text_map.GetLength();
            writer.text = new() { SizeOfRawData = length, VirtualSize = length };
            return writer;
        }

        private void BuildSections()
        {
            bool has_win32_resources = win32_resources != null;
            if (has_win32_resources)
                sections++;

            text = CreateSection(".text", text_map.GetLength(), null);
            Section previous = text;

            if (has_win32_resources)
            {
                rsrc = CreateSection(".rsrc", (uint)win32_resources.length, previous);

                PatchWin32Resources(win32_resources);
                previous = rsrc;
            }

            if (has_reloc)
                reloc = CreateSection(".reloc", 12u, previous);
        }

        private Section CreateSection(string name, uint size, Section previous)
        {
            return new()
            {
                Name = name,
                VirtualAddress = previous != null ? previous.VirtualAddress + Align(previous.VirtualSize, section_alignment) : text_rva,
                VirtualSize = size,
                PointerToRawData = previous != null ? previous.PointerToRawData + previous.SizeOfRawData : Align(GetHeaderSize(), file_alignment),
                SizeOfRawData = Align(size, file_alignment)
            };
        }

        private static uint Align(uint value, uint align)
        {
            align--;
            return (value + align) & ~align;
        }

        private void WriteDOSHeader()
        {
            Write(new byte[]
            {
                // dos header start
                0x4d, 0x5a, 0x90, 0x00, 0x03, 0x00, 0x00,
                0x00, 0x04, 0x00, 0x00, 0x00, 0xff, 0xff,
                0x00, 0x00, 0xb8, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
                // lfanew
                0x80, 0x00, 0x00, 0x00,
                // dos header end
                0x0e, 0x1f, 0xba, 0x0e, 0x00, 0xb4, 0x09,
                0xcd, 0x21, 0xb8, 0x01, 0x4c, 0xcd, 0x21,
                0x54, 0x68, 0x69, 0x73, 0x20, 0x70, 0x72,
                0x6f, 0x67, 0x72, 0x61, 0x6d, 0x20, 0x63,
                0x61, 0x6e, 0x6e, 0x6f, 0x74, 0x20, 0x62,
                0x65, 0x20, 0x72, 0x75, 0x6e, 0x20, 0x69,
                0x6e, 0x20, 0x44, 0x4f, 0x53, 0x20, 0x6d,
                0x6f, 0x64, 0x65, 0x2e, 0x0d, 0x0d, 0x0a,
                0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00
            });
        }

        private ushort SizeOfOptionalHeader()
        {
            return (ushort)(!pe64 ? 0xe0 : 0xf0);
        }

        private void WritePEFileHeader()
        {
            WriteUInt32(0x00004550); // Magic
            WriteUInt16((ushort)module.Architecture); // Machine
            WriteUInt16(sections); // NumberOfSections
            WriteUInt32(metadata.timestamp);
            WriteUInt32(0); // PointerToSymbolTable
            WriteUInt32(0); // NumberOfSymbols
            WriteUInt16(SizeOfOptionalHeader()); // SizeOfOptionalHeader

            const ushort LargeAddressAware = 0x0020;

            // ExecutableImage | (!pe64 ? 32BitsMachine : LargeAddressAware)
            ushort characteristics = (ushort)(0x0002 | (!pe64 ? 0x0100 : LargeAddressAware));
            if (module.Kind == ModuleKind.Dll || module.Kind == ModuleKind.NetModule)
                characteristics |= 0x2000;

            if (module.Image != null && (module.Image.Characteristics & LargeAddressAware) != 0)
                characteristics |= LargeAddressAware;

            WriteUInt16(characteristics); // Characteristics
        }

        private Section LastSection()
        {
            if (reloc != null)
                return reloc;

            if (rsrc != null)
                return rsrc;

            return text;
        }

        private void WriteOptionalHeaders()
        {
            WriteUInt16((ushort)(!pe64 ? 0x10b : 0x20b)); // Magic
            WriteUInt16(module.linker_version);
            WriteUInt32(text.SizeOfRawData); // CodeSize
            WriteUInt32((reloc != null ? reloc.SizeOfRawData : 0) + (rsrc != null ? rsrc.SizeOfRawData : 0)); // InitializedDataSize
            WriteUInt32(0); // UninitializedDataSize

            Range startub_stub = text_map.GetRange(TextSegment.StartupStub);
            WriteUInt32(startub_stub.Length > 0 ? startub_stub.Start : 0); // EntryPointRVA
            WriteUInt32(text_rva); // BaseOfCode

            if (!pe64)
            {
                WriteUInt32(0); // BaseOfData
                WriteUInt32((uint)image_base); // ImageBase
            }
            else
            {
                WriteUInt64(image_base); // ImageBase
            }

            WriteUInt32(section_alignment); // SectionAlignment
            WriteUInt32(file_alignment); // FileAlignment

            WriteUInt16(4); // OSMajor
            WriteUInt16(0); // OSMinor
            WriteUInt16(0); // UserMajor
            WriteUInt16(0); // UserMinor
            WriteUInt16(module.subsystem_major); // SubSysMajor
            WriteUInt16(module.subsystem_minor); // SubSysMinor
            WriteUInt32(0); // Reserved

            Section last_section = LastSection();
            WriteUInt32(last_section.VirtualAddress + Align(last_section.VirtualSize, section_alignment)); // ImageSize
            WriteUInt32(text.PointerToRawData); // HeaderSize

            WriteUInt32(0); // Checksum
            WriteUInt16(GetSubSystem()); // SubSystem
            WriteUInt16((ushort)module.Characteristics); // DLLFlags

            if (!pe64)
            {
                const uint stack_reserve = 0x100000;
                const uint stack_commit = 0x1000;
                const uint heap_reserve = 0x100000;
                const uint heap_commit = 0x1000;

                WriteUInt32(stack_reserve);
                WriteUInt32(stack_commit);
                WriteUInt32(heap_reserve);
                WriteUInt32(heap_commit);
            }
            else
            {
                const ulong stack_reserve = 0x400000;
                const ulong stack_commit = 0x4000;
                const ulong heap_reserve = 0x100000;
                const ulong heap_commit = 0x2000;

                WriteUInt64(stack_reserve);
                WriteUInt64(stack_commit);
                WriteUInt64(heap_reserve);
                WriteUInt64(heap_commit);
            }

            WriteUInt32(0); // LoaderFlags
            WriteUInt32(16); // NumberOfDataDir

            WriteZeroDataDirectory(); // ExportTable
            WriteDataDirectory(text_map.GetDataDirectory(TextSegment.ImportDirectory)); // ImportTable
            if (rsrc != null)
            {
                // ResourceTable
                WriteUInt32(rsrc.VirtualAddress);
                WriteUInt32(rsrc.VirtualSize);
            }
            else
            {
                WriteZeroDataDirectory();
            }

            WriteZeroDataDirectory(); // ExceptionTable
            WriteZeroDataDirectory(); // CertificateTable
            WriteUInt32(reloc != null ? reloc.VirtualAddress : 0); // BaseRelocationTable
            WriteUInt32(reloc != null ? reloc.VirtualSize : 0);

            if (text_map.GetLength(TextSegment.DebugDirectory) > 0)
            {
                WriteUInt32(text_map.GetRVA(TextSegment.DebugDirectory));
                WriteUInt32((uint)(debug_header.Entries.Length * ImageDebugDirectory.Size));
            }
            else
            {
                WriteZeroDataDirectory();
            }

            WriteZeroDataDirectory(); // Copyright
            WriteZeroDataDirectory(); // GlobalPtr
            WriteZeroDataDirectory(); // TLSTable
            WriteZeroDataDirectory(); // LoadConfigTable
            WriteZeroDataDirectory(); // BoundImport
            WriteDataDirectory(text_map.GetDataDirectory(TextSegment.ImportAddressTable)); // IAT
            WriteZeroDataDirectory(); // DelayImportDesc
            WriteDataDirectory(text_map.GetDataDirectory(TextSegment.CLIHeader)); // CLIHeader
            WriteZeroDataDirectory(); // Reserved
        }

        private void WriteZeroDataDirectory()
        {
            WriteUInt32(0);
            WriteUInt32(0);
        }

        private ushort GetSubSystem()
        {
            switch (module.Kind)
            {
                case ModuleKind.Console:
                case ModuleKind.Dll:
                case ModuleKind.NetModule:
                    return 0x3;
                case ModuleKind.Windows:
                    return 0x2;
                default:
                    throw new ArgumentOutOfRangeException();
            }
        }

        private void WriteSectionHeaders()
        {
            WriteSection(text, 0x60000020);

            if (rsrc != null)
                WriteSection(rsrc, 0x40000040);

            if (reloc != null)
                WriteSection(reloc, 0x42000040);
        }

        private void WriteSection(Section section, uint characteristics)
        {
            byte[] name = new byte [8];
            string sect_name = section.Name;
            for (int i = 0; i < sect_name.Length; i++)
                name[i] = (byte)sect_name[i];

            WriteBytes(name);
            WriteUInt32(section.VirtualSize);
            WriteUInt32(section.VirtualAddress);
            WriteUInt32(section.SizeOfRawData);
            WriteUInt32(section.PointerToRawData);
            WriteUInt32(0); // PointerToRelocations
            WriteUInt32(0); // PointerToLineNumbers
            WriteUInt16(0); // NumberOfRelocations
            WriteUInt16(0); // NumberOfLineNumbers
            WriteUInt32(characteristics);
        }

        private uint GetRVAFileOffset(Section section, RVA rva)
        {
            return section.PointerToRawData + rva - section.VirtualAddress;
        }

        private void MoveTo(uint pointer)
        {
            BaseStream.Seek(pointer, SeekOrigin.Begin);
        }

        private void MoveToRVA(Section section, RVA rva)
        {
            BaseStream.Seek(GetRVAFileOffset(section, rva), SeekOrigin.Begin);
        }

        private void MoveToRVA(TextSegment segment)
        {
            MoveToRVA(text, text_map.GetRVA(segment));
        }

        private void WriteRVA(RVA rva)
        {
            if (!pe64)
                WriteUInt32(rva);
            else
                WriteUInt64(rva);
        }

        private void PrepareSection(Section section)
        {
            MoveTo(section.PointerToRawData);

            const int buffer_size = 4096;

            if (section.SizeOfRawData <= buffer_size)
            {
                Write(new byte [section.SizeOfRawData]);
                MoveTo(section.PointerToRawData);
                return;
            }

            int written = 0;
            byte[] buffer = new byte [buffer_size];
            while (written != section.SizeOfRawData)
            {
                int write_size = Math.Min((int)section.SizeOfRawData - written, buffer_size);
                Write(buffer, 0, write_size);
                written += write_size;
            }

            MoveTo(section.PointerToRawData);
        }

        private void WriteText()
        {
            PrepareSection(text);

            // ImportAddressTable

            if (has_reloc)
            {
                WriteRVA(text_map.GetRVA(TextSegment.ImportHintNameTable));
                WriteRVA(0);
            }

            // CLIHeader

            WriteUInt32(0x48);
            WriteUInt16(2);
            WriteUInt16((ushort)(module.Runtime <= TargetRuntime.Net_1_1 ? 0 : 5));

            WriteUInt32(text_map.GetRVA(TextSegment.MetadataHeader));
            WriteUInt32(GetMetadataLength());
            WriteUInt32((uint)module.Attributes);
            WriteUInt32(metadata.entry_point.ToUInt32());
            WriteDataDirectory(text_map.GetDataDirectory(TextSegment.Resources));
            WriteDataDirectory(text_map.GetDataDirectory(TextSegment.StrongNameSignature));
            WriteZeroDataDirectory(); // CodeManagerTable
            WriteZeroDataDirectory(); // VTableFixups
            WriteZeroDataDirectory(); // ExportAddressTableJumps
            WriteZeroDataDirectory(); // ManagedNativeHeader

            // Code

            MoveToRVA(TextSegment.Code);
            WriteBuffer(metadata.code);

            // Resources

            MoveToRVA(TextSegment.Resources);
            WriteBuffer(metadata.resources);

            // Data

            if (metadata.data.length > 0)
            {
                MoveToRVA(TextSegment.Data);
                WriteBuffer(metadata.data);
            }

            // StrongNameSignature
            // stays blank

            // MetadataHeader

            MoveToRVA(TextSegment.MetadataHeader);
            WriteMetadataHeader();

            WriteMetadata();

            // DebugDirectory
            if (text_map.GetLength(TextSegment.DebugDirectory) > 0)
            {
                MoveToRVA(TextSegment.DebugDirectory);
                WriteDebugDirectory();
            }

            if (!has_reloc)
                return;

            // ImportDirectory
            MoveToRVA(TextSegment.ImportDirectory);
            WriteImportDirectory();

            // StartupStub
            MoveToRVA(TextSegment.StartupStub);
            WriteStartupStub();
        }

        private uint GetMetadataLength()
        {
            return text_map.GetRVA(TextSegment.DebugDirectory) - text_map.GetRVA(TextSegment.MetadataHeader);
        }

        public void WriteMetadataHeader()
        {
            WriteUInt32(0x424a5342); // Signature
            WriteUInt16(1); // MajorVersion
            WriteUInt16(1); // MinorVersion
            WriteUInt32(0); // Reserved

            byte[] version = GetZeroTerminatedString(runtime_version);
            WriteUInt32((uint)version.Length);
            WriteBytes(version);
            WriteUInt16(0); // Flags
            WriteUInt16(GetStreamCount());

            uint offset = text_map.GetRVA(TextSegment.TableHeap) - text_map.GetRVA(TextSegment.MetadataHeader);

            WriteStreamHeader(ref offset, TextSegment.TableHeap, "#~");
            WriteStreamHeader(ref offset, TextSegment.StringHeap, "#Strings");
            WriteStreamHeader(ref offset, TextSegment.UserStringHeap, "#US");
            WriteStreamHeader(ref offset, TextSegment.GuidHeap, "#GUID");
            WriteStreamHeader(ref offset, TextSegment.BlobHeap, "#Blob");
            WriteStreamHeader(ref offset, TextSegment.PdbHeap, "#Pdb");
        }

        private ushort GetStreamCount()
        {
            return (ushort)(1 // #~
                            + 1 // #Strings
                            + (metadata.user_string_heap.IsEmpty ? 0 : 1) // #US
                            + (metadata.guid_heap.IsEmpty ? 0 : 1) // GUID
                            + (metadata.blob_heap.IsEmpty ? 0 : 1) + (metadata.pdb_heap == null ? 0 : 1)); // #Blob
        }

        private void WriteStreamHeader(ref uint offset, TextSegment heap, string name)
        {
            uint length = (uint)text_map.GetLength(heap);
            if (length == 0)
                return;

            WriteUInt32(offset);
            WriteUInt32(length);
            WriteBytes(GetZeroTerminatedString(name));
            offset += length;
        }

        private static int GetZeroTerminatedStringLength(string @string)
        {
            return (@string.Length + 1 + 3) & ~3;
        }

        private static byte[] GetZeroTerminatedString(string @string)
        {
            return GetString(@string, GetZeroTerminatedStringLength(@string));
        }

        private static byte[] GetSimpleString(string @string)
        {
            return GetString(@string, @string.Length);
        }

        private static byte[] GetString(string @string, int length)
        {
            byte[] bytes = new byte [length];
            for (int i = 0; i < @string.Length; i++)
                bytes[i] = (byte)@string[i];

            return bytes;
        }

        public void WriteMetadata()
        {
            WriteHeap(TextSegment.TableHeap, metadata.table_heap);
            WriteHeap(TextSegment.StringHeap, metadata.string_heap);
            WriteHeap(TextSegment.UserStringHeap, metadata.user_string_heap);
            WriteHeap(TextSegment.GuidHeap, metadata.guid_heap);
            WriteHeap(TextSegment.BlobHeap, metadata.blob_heap);
            WriteHeap(TextSegment.PdbHeap, metadata.pdb_heap);
        }

        private void WriteHeap(TextSegment heap, HeapBuffer buffer)
        {
            if (buffer == null || buffer.IsEmpty)
                return;

            MoveToRVA(heap);
            WriteBuffer(buffer);
        }

        private void WriteDebugDirectory()
        {
            int data_start = (int)BaseStream.Position + debug_header.Entries.Length * ImageDebugDirectory.Size;

            for (int i = 0; i < debug_header.Entries.Length; i++)
            {
                ImageDebugHeaderEntry entry = debug_header.Entries[i];
                ImageDebugDirectory directory = entry.Directory;
                WriteInt32(directory.Characteristics);
                WriteInt32(directory.TimeDateStamp);
                WriteInt16(directory.MajorVersion);
                WriteInt16(directory.MinorVersion);
                WriteInt32((int)directory.Type);
                WriteInt32(directory.SizeOfData);
                WriteInt32(directory.AddressOfRawData);
                WriteInt32(data_start);

                data_start += entry.Data.Length;
            }

            for (int i = 0; i < debug_header.Entries.Length; i++)
            {
                ImageDebugHeaderEntry entry = debug_header.Entries[i];
                WriteBytes(entry.Data);
            }
        }

        private void WriteImportDirectory()
        {
            WriteUInt32(text_map.GetRVA(TextSegment.ImportDirectory) + 40); // ImportLookupTable
            WriteUInt32(0); // DateTimeStamp
            WriteUInt32(0); // ForwarderChain
            WriteUInt32(text_map.GetRVA(TextSegment.ImportHintNameTable) + 14);
            WriteUInt32(text_map.GetRVA(TextSegment.ImportAddressTable));
            Advance(20);

            // ImportLookupTable
            WriteUInt32(text_map.GetRVA(TextSegment.ImportHintNameTable));

            // ImportHintNameTable
            MoveToRVA(TextSegment.ImportHintNameTable);

            WriteUInt16(0); // Hint
            WriteBytes(GetRuntimeMain());
            WriteByte(0);
            WriteBytes(GetSimpleString("mscoree.dll"));
            WriteUInt16(0);
        }

        private byte[] GetRuntimeMain()
        {
            return module.Kind == ModuleKind.Dll || module.Kind == ModuleKind.NetModule ? GetSimpleString("_CorDllMain") : GetSimpleString("_CorExeMain");
        }

        private void WriteStartupStub()
        {
            switch (module.Architecture)
            {
                case TargetArchitecture.I386:
                    WriteUInt16(0x25ff);
                    WriteUInt32((uint)image_base + text_map.GetRVA(TextSegment.ImportAddressTable));
                    return;
                default:
                    throw new NotSupportedException();
            }
        }

        private void WriteRsrc()
        {
            PrepareSection(rsrc);
            WriteBuffer(win32_resources);
        }

        private void WriteReloc()
        {
            PrepareSection(reloc);

            uint reloc_rva = text_map.GetRVA(TextSegment.StartupStub);
            reloc_rva += module.Architecture == TargetArchitecture.IA64 ? 0x20u : 2;
            uint page_rva = reloc_rva & ~0xfffu;

            WriteUInt32(page_rva); // PageRVA
            WriteUInt32(0x000c); // Block Size

            switch (module.Architecture)
            {
                case TargetArchitecture.I386:
                    WriteUInt32(0x3000 + reloc_rva - page_rva);
                    break;
                default:
                    throw new NotSupportedException();
            }
        }

        public void WriteImage()
        {
            WriteDOSHeader();
            WritePEFileHeader();
            WriteOptionalHeaders();
            WriteSectionHeaders();
            WriteText();
            if (rsrc != null)
                WriteRsrc();
            if (reloc != null)
                WriteReloc();
            Flush();
        }

        private void BuildTextMap()
        {
            TextMap map = text_map;

            map.AddMap(TextSegment.Code, metadata.code.length, !pe64 ? 4 : 16);
            map.AddMap(TextSegment.Resources, metadata.resources.length, 8);
            map.AddMap(TextSegment.Data, metadata.data.length, 4);
            if (metadata.data.length > 0)
                metadata.table_heap.FixupData(map.GetRVA(TextSegment.Data));
            map.AddMap(TextSegment.StrongNameSignature, GetStrongNameLength(), 4);

            BuildMetadataTextMap();

            int debug_dir_len = 0;
            if (debug_header != null && debug_header.HasEntries)
            {
                int directories_len = debug_header.Entries.Length * ImageDebugDirectory.Size;
                int data_address = (int)map.GetNextRVA(TextSegment.BlobHeap) + directories_len;
                int data_len = 0;

                for (int i = 0; i < debug_header.Entries.Length; i++)
                {
                    ImageDebugHeaderEntry entry = debug_header.Entries[i];
                    ImageDebugDirectory directory = entry.Directory;

                    directory.AddressOfRawData = entry.Data.Length == 0 ? 0 : data_address;
                    entry.Directory = directory;

                    data_len += entry.Data.Length;
                    data_address += data_len;
                }

                debug_dir_len = directories_len + data_len;
            }

            map.AddMap(TextSegment.DebugDirectory, debug_dir_len, 4);

            if (!has_reloc)
            {
                uint start = map.GetNextRVA(TextSegment.DebugDirectory);
                map.AddMap(TextSegment.ImportDirectory, new Range(start, 0));
                map.AddMap(TextSegment.ImportHintNameTable, new Range(start, 0));
                map.AddMap(TextSegment.StartupStub, new Range(start, 0));
                return;
            }

            RVA import_dir_rva = map.GetNextRVA(TextSegment.DebugDirectory);
            RVA import_hnt_rva = import_dir_rva + 48u;
            import_hnt_rva = (import_hnt_rva + 15u) & ~15u;
            uint import_dir_len = import_hnt_rva - import_dir_rva + 27u;

            RVA startup_stub_rva = import_dir_rva + import_dir_len;
            startup_stub_rva = module.Architecture == TargetArchitecture.IA64 ? (startup_stub_rva + 15u) & ~15u : 2 + ((startup_stub_rva + 3u) & ~3u);

            map.AddMap(TextSegment.ImportDirectory, new Range(import_dir_rva, import_dir_len));
            map.AddMap(TextSegment.ImportHintNameTable, new Range(import_hnt_rva, 0));
            map.AddMap(TextSegment.StartupStub, new Range(startup_stub_rva, GetStartupStubLength()));
        }

        public void BuildMetadataTextMap()
        {
            TextMap map = text_map;

            map.AddMap(TextSegment.MetadataHeader, GetMetadataHeaderLength(module.RuntimeVersion));
            map.AddMap(TextSegment.TableHeap, metadata.table_heap.length, 4);
            map.AddMap(TextSegment.StringHeap, metadata.string_heap.length, 4);
            map.AddMap(TextSegment.UserStringHeap, metadata.user_string_heap.IsEmpty ? 0 : metadata.user_string_heap.length, 4);
            map.AddMap(TextSegment.GuidHeap, metadata.guid_heap.length, 4);
            map.AddMap(TextSegment.BlobHeap, metadata.blob_heap.IsEmpty ? 0 : metadata.blob_heap.length, 4);
            map.AddMap(TextSegment.PdbHeap, metadata.pdb_heap == null ? 0 : metadata.pdb_heap.length, 4);
        }

        private uint GetStartupStubLength()
        {
            switch (module.Architecture)
            {
                case TargetArchitecture.I386:
                    return 6;
                default:
                    throw new NotSupportedException();
            }
        }

        private int GetMetadataHeaderLength(string runtimeVersion)
        {
            return
                // MetadataHeader
                20 + GetZeroTerminatedStringLength(runtimeVersion)
                   // #~ header
                   + 12
                   // #Strings header
                   + 20
                   // #US header
                   + (metadata.user_string_heap.IsEmpty ? 0 : 12)
                   // #GUID header
                   + 16
                   // #Blob header
                   + (metadata.blob_heap.IsEmpty ? 0 : 16)
                   //
                   + (metadata.pdb_heap == null ? 0 : 16);
        }

        private int GetStrongNameLength()
        {
            if (module.kind == ModuleKind.NetModule || module.Assembly == null)
                return 0;

            byte[] public_key = module.Assembly.Name.PublicKey;
            if (public_key.IsNullOrEmpty())
                return 0;

            // in fx 2.0 the key may be from 384 to 16384 bits
            // so we must calculate the signature size based on
            // the size of the public key (minus the 32 byte header)
            int size = public_key.Length;
            if (size > 32)
                return size - 32;

            // note: size == 16 for the ECMA "key" which is replaced
            // by the runtime with a 1024 bits key (128 bytes)

            return 128; // default strongname signature size
        }

        public DataDirectory GetStrongNameSignatureDirectory()
        {
            return text_map.GetDataDirectory(TextSegment.StrongNameSignature);
        }

        public uint GetHeaderSize()
        {
            return pe_header_size + SizeOfOptionalHeader() + sections * section_header_size;
        }

        private void PatchWin32Resources(ByteBuffer resources)
        {
            PatchResourceDirectoryTable(resources);
        }

        private void PatchResourceDirectoryTable(ByteBuffer resources)
        {
            resources.Advance(12);

            int entries = resources.ReadUInt16() + resources.ReadUInt16();

            for (int i = 0; i < entries; i++)
                PatchResourceDirectoryEntry(resources);
        }

        private void PatchResourceDirectoryEntry(ByteBuffer resources)
        {
            resources.Advance(4);
            uint child = resources.ReadUInt32();

            int position = resources.position;
            resources.position = (int)child & 0x7fffffff;

            if ((child & 0x80000000) != 0)
                PatchResourceDirectoryTable(resources);
            else
                PatchResourceDataEntry(resources);

            resources.position = position;
        }

        private void PatchResourceDataEntry(ByteBuffer resources)
        {
            uint rva = resources.ReadUInt32();
            resources.position -= 4;

            resources.WriteUInt32(rva - module.Image.Win32Resources.VirtualAddress + rsrc.VirtualAddress);
        }
    }
}