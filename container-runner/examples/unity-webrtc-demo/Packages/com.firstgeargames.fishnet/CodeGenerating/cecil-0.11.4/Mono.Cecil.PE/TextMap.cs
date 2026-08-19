//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using RVA = System.UInt32;

namespace MonoFN.Cecil.PE
{
    internal enum TextSegment
    {
        ImportAddressTable,
        CLIHeader,
        Code,
        Resources,
        Data,
        StrongNameSignature,

        // Metadata
        MetadataHeader,
        TableHeap,
        StringHeap,
        UserStringHeap,
        GuidHeap,
        BlobHeap,
        PdbHeap,
        // End Metadata
        DebugDirectory,
        ImportDirectory,
        ImportHintNameTable,
        StartupStub
    }

    internal sealed class TextMap
    {
        private readonly Range[] map = new Range [17 /*Enum.GetValues (typeof (TextSegment)).Length*/];

        public void AddMap(TextSegment segment, int length)
        {
            map[(int)segment] = new(GetStart(segment), (uint)length);
        }

        public void AddMap(TextSegment segment, int length, int align)
        {
            align--;

            AddMap(segment, (length + align) & ~align);
        }

        public void AddMap(TextSegment segment, Range range)
        {
            map[(int)segment] = range;
        }

        public Range GetRange(TextSegment segment)
        {
            return map[(int)segment];
        }

        public DataDirectory GetDataDirectory(TextSegment segment)
        {
            Range range = map[(int)segment];

            return new(range.Length == 0 ? 0 : range.Start, range.Length);
        }

        public RVA GetRVA(TextSegment segment)
        {
            return map[(int)segment].Start;
        }

        public RVA GetNextRVA(TextSegment segment)
        {
            int i = (int)segment;
            return map[i].Start + map[i].Length;
        }

        public int GetLength(TextSegment segment)
        {
            return (int)map[(int)segment].Length;
        }

        private RVA GetStart(TextSegment segment)
        {
            int index = (int)segment;
            return index == 0 ? ImageWriter.text_rva : ComputeStart(index);
        }

        private RVA ComputeStart(int index)
        {
            index--;
            return map[index].Start + map[index].Length;
        }

        public uint GetLength()
        {
            Range range = map[(int)TextSegment.StartupStub];
            return range.Start - ImageWriter.text_rva + range.Length;
        }
    }
}