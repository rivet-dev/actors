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
    internal sealed class Image : IDisposable
    {
        public Disposable<Stream> Stream;
        public string FileName;
        public ModuleKind Kind;
        public uint Characteristics;
        public string RuntimeVersion;
        public TargetArchitecture Architecture;
        public ModuleCharacteristics DllCharacteristics;
        public ushort LinkerVersion;
        public ushort SubSystemMajor;
        public ushort SubSystemMinor;
        public ImageDebugHeader DebugHeader;
        public Section[] Sections;
        public Section MetadataSection;
        public uint EntryPointToken;
        public uint Timestamp;
        public ModuleAttributes Attributes;
        public DataDirectory Win32Resources;
        public DataDirectory Debug;
        public DataDirectory Resources;
        public DataDirectory StrongName;
        public StringHeap StringHeap;
        public BlobHeap BlobHeap;
        public UserStringHeap UserStringHeap;
        public GuidHeap GuidHeap;
        public TableHeap TableHeap;
        public PdbHeap PdbHeap;
        private readonly int[] coded_index_sizes = new int [14];
        private readonly Func<Table, int> counter;

        public Image()
        {
            counter = GetTableLength;
        }

        public bool HasTable(Table table)
        {
            return GetTableLength(table) > 0;
        }

        public int GetTableLength(Table table)
        {
            return (int)TableHeap[table].Length;
        }

        public int GetTableIndexSize(Table table)
        {
            return GetTableLength(table) < 65536 ? 2 : 4;
        }

        public int GetCodedIndexSize(CodedIndex coded_index)
        {
            int index = (int)coded_index;
            int size = coded_index_sizes[index];
            if (size != 0)
                return size;

            return coded_index_sizes[index] = coded_index.GetSize(counter);
        }

        public uint ResolveVirtualAddress(RVA rva)
        {
            Section section = GetSectionAtVirtualAddress(rva);
            if (section == null)
                throw new ArgumentOutOfRangeException();

            return ResolveVirtualAddressInSection(rva, section);
        }

        public uint ResolveVirtualAddressInSection(RVA rva, Section section)
        {
            return rva + section.PointerToRawData - section.VirtualAddress;
        }

        public Section GetSection(string name)
        {
            Section[] sections = Sections;
            for (int i = 0; i < sections.Length; i++)
            {
                Section section = sections[i];
                if (section.Name == name)
                    return section;
            }

            return null;
        }

        public Section GetSectionAtVirtualAddress(RVA rva)
        {
            Section[] sections = Sections;
            for (int i = 0; i < sections.Length; i++)
            {
                Section section = sections[i];
                if (rva >= section.VirtualAddress && rva < section.VirtualAddress + section.SizeOfRawData)
                    return section;
            }

            return null;
        }

        private BinaryStreamReader GetReaderAt(RVA rva)
        {
            Section section = GetSectionAtVirtualAddress(rva);
            if (section == null)
                return null;

            BinaryStreamReader reader = new(Stream.value);
            reader.MoveTo(ResolveVirtualAddressInSection(rva, section));
            return reader;
        }

        public TRet GetReaderAt<TItem, TRet>(RVA rva, TItem item, Func<TItem, BinaryStreamReader, TRet> read) where TRet : class
        {
            long position = Stream.value.Position;
            try
            {
                BinaryStreamReader reader = GetReaderAt(rva);
                if (reader == null)
                    return null;

                return read(item, reader);
            }
            finally
            {
                Stream.value.Position = position;
            }
        }

        public bool HasDebugTables()
        {
            return HasTable(Table.Document) || HasTable(Table.MethodDebugInformation) || HasTable(Table.LocalScope) || HasTable(Table.LocalVariable) || HasTable(Table.LocalConstant) || HasTable(Table.StateMachineMethod) || HasTable(Table.CustomDebugInformation);
        }

        public void Dispose()
        {
            Stream.Dispose();
        }
    }
}