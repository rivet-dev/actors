//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

namespace MonoFN.Cecil
{
    public class ModuleReference : IMetadataScope
    {
        internal MetadataToken token;
        public string Name { get; set; }
        public virtual MetadataScopeType MetadataScopeType
        {
            get { return MetadataScopeType.ModuleReference; }
        }
        public MetadataToken MetadataToken
        {
            get { return token; }
            set { token = value; }
        }

        internal ModuleReference()
        {
            token = new(TokenType.ModuleRef);
        }

        public ModuleReference(string name) : this()
        {
            this.Name = name;
        }

        public override string ToString()
        {
            return Name;
        }
    }
}