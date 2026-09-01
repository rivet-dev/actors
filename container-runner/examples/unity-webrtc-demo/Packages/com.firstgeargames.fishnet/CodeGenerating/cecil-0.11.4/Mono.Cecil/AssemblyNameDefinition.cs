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

namespace MonoFN.Cecil
{
    public sealed class AssemblyNameDefinition : AssemblyNameReference
    {
        public override byte[] Hash
        {
            get { return Empty<byte>.Array; }
        }

        internal AssemblyNameDefinition()
        {
            token = new(TokenType.Assembly, 1);
        }

        public AssemblyNameDefinition(string name, Version version) : base(name, version)
        {
            token = new(TokenType.Assembly, 1);
        }
    }
}