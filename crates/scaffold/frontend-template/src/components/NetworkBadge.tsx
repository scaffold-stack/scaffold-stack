import React from 'react'
import { scaffoldConfig } from '../scaffold.config';

function NetworkBadge() {
    const network = scaffoldConfig.network;
    return (
      <span className={`text-xs px-2 py-0.5 font-mono text-[#8F8D8E]`}>
        {network}
      </span>
    );
  }

export default NetworkBadge