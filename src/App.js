import React from 'react';
import styled, { createGlobalStyle } from 'styled-components';

const GlobalStyle = createGlobalStyle`
  :root {
    --bg: #0f0f0f;
    --card-bg: #1a1a1a;
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    padding: 2rem 1rem;
  }

  a {
    color: var(--hover);
    text-decoration: none;

    &:hover {
      text-decoration: underline;
    }
  }
`;

const Container = styled.div`
  max-width: 1200px;
  margin: 0 auto;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 3rem;

  h1 {
    color: var(--hover);
    font-size: 2.5rem;
    margin-bottom: 1rem;
  }

  p {
    max-width: 600px;
    margin: 0 auto;
  }
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 1rem;
  justify-content: center;
  margin: 2rem 0;

  @media (max-width: 768px) {
    flex-direction: column;
  }
`;

const Button = styled.a`
  padding: 0.8rem 1.5rem;
  border-radius: 6px;
  text-decoration: none;
  font-weight: 500;
  transition: 0.2s all ease;
  cursor: pointer;

  &.primary {
    background: var(--accent);
    color: white;

    &:hover {
      background: var(--hover);
    }
  }

  &.secondary {
    border: 1px solid var(--accent);
    color: var(--accent);

    &:hover {
      border-color: var(--hover);
      color: var(--hover);
    }
  }
`;

const Section = styled.section`
  background: var(--card-bg);
  border-radius: 8px;
  padding: 2rem;
  margin-bottom: 2rem;

  h2 {
    color: var(--accent);
    margin-bottom: 1.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #333;
  }
`;

const FeatureGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 1.5rem;
  margin-top: 1.5rem;
`;

const Feature = styled.div`
  padding: 1.25rem;
  background: var(--bg);
  border-radius: 8px;
  border: 1px solid #333;

  h3 {
    margin-bottom: 0.75rem;
    color: var(--hover);
  }

  ul {
    padding-left: 1.25rem;
  }

  li {
    margin-bottom: 0.5rem;
  }
`;

const Highlight = styled.span`
  color: var(--hover);
  font-weight: 500;
`;

const Credits = styled.footer`
  opacity: 0.8;
  font-size: 0.9rem;
  margin-top: 2rem;
  padding-top: 2rem;
  border-top: 1px solid #333;

  a {
    color: var(--hover);
  }
`;

const App = () => {
  return (
    <>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Open-source Windows automation tool for Roblox - works externally without game modifications</p>
          
          <ButtonGroup>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" 
              className="primary"
            >
              Download Portable EXE
            </Button>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities" 
              className="secondary"
            >
              View Source Code
            </Button>
          </ButtonGroup>
        </Header>

        <Section>
          <h2>Key Features</h2>
          <FeatureGrid>
            <Feature>
              <h3>Customization</h3>
              <ul>
                <li><Highlight>Drag-and-drop UI</Highlight> - Rearrange buttons freely</li>
                <li>Automatically saves your layout</li>
                <li>Clean, minimal interface</li>
              </ul>
            </Feature>
            
            <Feature>
              <h3>Convenience</h3>
              <ul>
                <li>Persistent anti-AFK</li>
                <li>Single executable file</li>
                <li>No installation required</li>
                <li>Windows 10/11 supported</li>
              </ul>
            </Feature>

            <Feature>
              <h3>Gameplay Macros</h3>
              <ul>
                <li>Speedglitch toggle</li>
                <li>High jump macros</li>
                <li>Ledge bouncing</li>
                <li>Wallhop/Wall-walk</li>
                <li>Item clipping</li>
                <li>+10 more specialized actions</li>
              </ul>
            </Feature>
          </FeatureGrid>
        </Section>

        <Section>
          <h2>Troubleshooting</h2>
          <FeatureGrid>
            <Feature>
              <h3>Common Solutions</h3>
              <ul>
                <li><strong>DLL errors:</strong> Install <a href="https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist">Visual C++ Redist</a></li>
                <li><strong>Keybind issues:</strong> Full system restart</li>
                <li><strong>Launch problems:</strong> Right-click → Properties → Unblock</li>
              </ul>
            </Feature>
            
            <Feature>
              <h3>Technical Notes</h3>
              <ul>
                <li>Doesn't modify Roblox memory</li>
                <li>No background services</li>
                <li>Settings stored in local config</li>
                <li>Built with C++/ImGui</li>
              </ul>
            </Feature>
          </FeatureGrid>
        </Section>

        <Section>
          <h2>Support & Community</h2>
          <div style={{ marginTop: '1.5rem' }}>
            <p style={{ marginBottom: '1rem' }}>
              Get help or discuss glitches:
            </p>
            <Button 
              href="https://discord.gg/roblox-glitching-community-998572881892094012" 
              className="secondary"
              style={{ marginTop: '0.5rem' }}
            >
              Join Discord Server
            </Button>
          </div>
        </Section>

        <Credits>
          <p>Built with:</p>
          <ul>
            <li><a href="https://github.com/ocornut/imgui">ImGui</a> interface framework</li>
            <li><a href="https://github.com/craftwar/suspend">suspend</a> core functionality</li>
          </ul>
        </Credits>
      </Container>
    </>
  );
};

export default App;
