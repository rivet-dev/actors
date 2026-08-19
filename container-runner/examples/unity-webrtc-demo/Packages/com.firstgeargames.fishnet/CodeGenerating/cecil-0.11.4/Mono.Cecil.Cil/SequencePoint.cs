//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;

namespace MonoFN.Cecil.Cil
{
    public sealed class SequencePoint
    {
        internal InstructionOffset offset;
        public int Offset
        {
            get { return offset.Offset; }
        }
        public int StartLine { get; set; }
        public int StartColumn { get; set; }
        public int EndLine { get; set; }
        public int EndColumn { get; set; }
        public bool IsHidden
        {
            get { return StartLine == 0xfeefee && StartLine == EndLine; }
        }
        public Document Document { get; set; }

        internal SequencePoint(int offset, Document document)
        {
            if (document == null)
                throw new ArgumentNullException("document");

            this.offset = new(offset);
            this.Document = document;
        }

        public SequencePoint(Instruction instruction, Document document)
        {
            if (document == null)
                throw new ArgumentNullException("document");

            offset = new(instruction);
            this.Document = document;
        }
    }
}